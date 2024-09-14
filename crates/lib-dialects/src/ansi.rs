use std::sync::Arc;

use itertools::Itertools;
use sqruff_lib_core::dialects::base::Dialect;
use sqruff_lib_core::dialects::syntax::SyntaxKind;
use sqruff_lib_core::helpers::{Config, ToMatchable};
use sqruff_lib_core::parser::grammar::anyof::{one_of, optionally_bracketed, AnyNumberOf};
use sqruff_lib_core::parser::grammar::base::{Anything, Nothing, Ref};
use sqruff_lib_core::parser::grammar::conditional::Conditional;
use sqruff_lib_core::parser::grammar::delimited::Delimited;
use sqruff_lib_core::parser::grammar::sequence::{Bracketed, Sequence};
use sqruff_lib_core::parser::lexer::{Matcher, Pattern};
use sqruff_lib_core::parser::matchable::Matchable;
use sqruff_lib_core::parser::node_matcher::NodeMatcher;
use sqruff_lib_core::parser::parsers::{MultiStringParser, RegexParser, StringParser, TypedParser};
use sqruff_lib_core::parser::segments::bracketed::BracketedSegmentMatcher;
use sqruff_lib_core::parser::segments::generator::SegmentGenerator;
use sqruff_lib_core::parser::segments::meta::MetaSegment;
use sqruff_lib_core::parser::types::ParseMode;
use sqruff_lib_core::vec_of_erased;

use super::ansi_keywords::{ANSI_RESERVED_KEYWORDS, ANSI_UNRESERVED_KEYWORDS};

trait BoxedE {
    fn boxed(self) -> Arc<Self>;
}

impl<T> BoxedE for T {
    fn boxed(self) -> Arc<Self>
    where
        Self: Sized,
    {
        Arc::new(self)
    }
}

pub fn dialect() -> Dialect {
    raw_dialect().config(|this| this.expand())
}

pub fn raw_dialect() -> Dialect {
    let mut ansi_dialect = Dialect::new("FileSegment");

    ansi_dialect.set_lexer_matchers(lexer_matchers());

    // Set the bare functions
    ansi_dialect.sets_mut("bare_functions").extend([
        "current_timestamp",
        "current_time",
        "current_date",
    ]);

    // Set the datetime units
    ansi_dialect.sets_mut("datetime_units").extend([
        "DAY",
        "DAYOFYEAR",
        "HOUR",
        "MILLISECOND",
        "MINUTE",
        "MONTH",
        "QUARTER",
        "SECOND",
        "WEEK",
        "WEEKDAY",
        "YEAR",
    ]);

    ansi_dialect.sets_mut("date_part_function_name").extend(["DATEADD"]);

    // Set Keywords
    ansi_dialect
        .update_keywords_set_from_multiline_string("unreserved_keywords", ANSI_UNRESERVED_KEYWORDS);
    ansi_dialect
        .update_keywords_set_from_multiline_string("reserved_keywords", ANSI_RESERVED_KEYWORDS);

    // Bracket pairs (a set of tuples).
    // (name, startref, endref, persists)
    // NOTE: The `persists` value controls whether this type
    // of bracket is persisted during matching to speed up other
    // parts of the matching process. Round brackets are the most
    // common and match the largest areas and so are sufficient.
    ansi_dialect.update_bracket_sets(
        "bracket_pairs",
        vec![
            ("round", "StartBracketSegment", "EndBracketSegment", true),
            ("square", "StartSquareBracketSegment", "EndSquareBracketSegment", false),
            ("curly", "StartCurlyBracketSegment", "EndCurlyBracketSegment", false),
        ],
    );

    // Set the value table functions. These are functions that, if they appear as
    // an item in "FROM", are treated as returning a COLUMN, not a TABLE.
    // Apparently, among dialects supported by SQLFluff, only BigQuery has this
    // concept, but this set is defined in the ANSI dialect because:
    // - It impacts core linter rules (see AL04 and several other rules that
    //   subclass from it) and how they interpret the contents of table_expressions
    // - At least one other database (DB2) has the same value table function,
    //   UNNEST(), as BigQuery. DB2 is not currently supported by SQLFluff.
    ansi_dialect.sets_mut("value_table_functions");

    ansi_dialect.add([
        (
            "ArrayTypeSchemaSegment".into(),
            NodeMatcher::new(SyntaxKind::ArrayType, Nothing::new().to_matchable())
                .to_matchable()
                .into(),
        ),
        (
            "ObjectReferenceSegment".into(),
            NodeMatcher::new(
                SyntaxKind::ObjectReference,
                Delimited::new(vec![Ref::new("SingleIdentifierGrammar").boxed()])
                    .config(|this| {
                        this.delimiter(Ref::new("ObjectReferenceDelimiterGrammar"));
                        this.disallow_gaps();
                        this.terminators =
                            vec_of_erased![Ref::new("ObjectReferenceTerminatorGrammar")];
                    })
                    .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
    ]);

    ansi_dialect.add([
        // Real segments
        ("DelimiterGrammar".into(), Ref::new("SemicolonSegment").to_matchable().into()),
        (
            "SemicolonSegment".into(),
            StringParser::new(";", SyntaxKind::StatementTerminator).to_matchable().into(),
        ),
        ("ColonSegment".into(), StringParser::new(":", SyntaxKind::Colon).to_matchable().into()),
        ("SliceSegment".into(), StringParser::new(":", SyntaxKind::Slice).to_matchable().into()),
        // NOTE: The purpose of the colon_delimiter is that it has different layout rules.
        // It assumes no whitespace on either side.
        (
            "ColonDelimiterSegment".into(),
            StringParser::new(":", SyntaxKind::ColonDelimiter).to_matchable().into(),
        ),
        (
            "StartBracketSegment".into(),
            StringParser::new("(", SyntaxKind::StartBracket).to_matchable().into(),
        ),
        (
            "EndBracketSegment".into(),
            StringParser::new(")", SyntaxKind::EndBracket).to_matchable().into(),
        ),
        (
            "StartSquareBracketSegment".into(),
            StringParser::new("[", SyntaxKind::StartSquareBracket).to_matchable().into(),
        ),
        (
            "EndSquareBracketSegment".into(),
            StringParser::new("]", SyntaxKind::EndSquareBracket).to_matchable().into(),
        ),
        (
            "StartCurlyBracketSegment".into(),
            StringParser::new("{", SyntaxKind::StartCurlyBracket).to_matchable().into(),
        ),
        (
            "EndCurlyBracketSegment".into(),
            StringParser::new("}", SyntaxKind::EndCurlyBracket).to_matchable().into(),
        ),
        ("CommaSegment".into(), StringParser::new(",", SyntaxKind::Comma).to_matchable().into()),
        ("DotSegment".into(), StringParser::new(".", SyntaxKind::Dot).to_matchable().into()),
        ("StarSegment".into(), StringParser::new("*", SyntaxKind::Star).to_matchable().into()),
        ("TildeSegment".into(), StringParser::new("~", SyntaxKind::Tilde).to_matchable().into()),
        (
            "ParameterSegment".into(),
            StringParser::new("?", SyntaxKind::Parameter).to_matchable().into(),
        ),
        (
            "CastOperatorSegment".into(),
            StringParser::new("::", SyntaxKind::CastingOperator).to_matchable().into(),
        ),
        (
            "PlusSegment".into(),
            StringParser::new("+", SyntaxKind::BinaryOperator).to_matchable().into(),
        ),
        (
            "MinusSegment".into(),
            StringParser::new("-", SyntaxKind::BinaryOperator).to_matchable().into(),
        ),
        (
            "PositiveSegment".into(),
            StringParser::new("+", SyntaxKind::SignIndicator).to_matchable().into(),
        ),
        (
            "NegativeSegment".into(),
            StringParser::new("-", SyntaxKind::SignIndicator).to_matchable().into(),
        ),
        (
            "DivideSegment".into(),
            StringParser::new("/", SyntaxKind::BinaryOperator).to_matchable().into(),
        ),
        (
            "MultiplySegment".into(),
            StringParser::new("*", SyntaxKind::BinaryOperator).to_matchable().into(),
        ),
        (
            "ModuloSegment".into(),
            StringParser::new("%", SyntaxKind::BinaryOperator).to_matchable().into(),
        ),
        ("SlashSegment".into(), StringParser::new("/", SyntaxKind::Slash).to_matchable().into()),
        (
            "AmpersandSegment".into(),
            StringParser::new("&", SyntaxKind::Ampersand).to_matchable().into(),
        ),
        ("PipeSegment".into(), StringParser::new("|", SyntaxKind::Pipe).to_matchable().into()),
        (
            "BitwiseXorSegment".into(),
            StringParser::new("^", SyntaxKind::BinaryOperator).to_matchable().into(),
        ),
        (
            "LikeOperatorSegment".into(),
            TypedParser::new(SyntaxKind::LikeOperator, SyntaxKind::ComparisonOperator)
                .to_matchable()
                .into(),
        ),
        (
            "RawNotSegment".into(),
            StringParser::new("!", SyntaxKind::RawComparisonOperator).to_matchable().into(),
        ),
        (
            "RawEqualsSegment".into(),
            StringParser::new("=", SyntaxKind::RawComparisonOperator).to_matchable().into(),
        ),
        (
            "RawGreaterThanSegment".into(),
            StringParser::new(">", SyntaxKind::RawComparisonOperator).to_matchable().into(),
        ),
        (
            "RawLessThanSegment".into(),
            StringParser::new("<", SyntaxKind::RawComparisonOperator).to_matchable().into(),
        ),
        (
            // The following functions can be called without parentheses per ANSI specification
            "BareFunctionSegment".into(),
            SegmentGenerator::new(|dialect| {
                MultiStringParser::new(
                    dialect.sets("bare_functions").into_iter().map(Into::into).collect_vec(),
                    SyntaxKind::BareFunction,
                )
                .boxed()
            })
            .into(),
        ),
        // The strange regex here it to make sure we don't accidentally match numeric
        // literals. We also use a regex to explicitly exclude disallowed keywords.
        (
            "NakedIdentifierSegment".into(),
            SegmentGenerator::new(|dialect| {
                // Generate the anti template from the set of reserved keywords
                let reserved_keywords = dialect.sets("reserved_keywords");
                let pattern = reserved_keywords.iter().join("|");
                let anti_template = format!("^({})$", pattern);

                RegexParser::new("[A-Z0-9_]*[A-Z][A-Z0-9_]*", SyntaxKind::NakedIdentifier)
                    .anti_template(&anti_template)
                    .boxed()
            })
            .into(),
        ),
        (
            "ParameterNameSegment".into(),
            RegexParser::new(r#"\"?[A-Z][A-Z0-9_]*\"?"#, SyntaxKind::Parameter)
                .to_matchable()
                .into(),
        ),
        (
            "FunctionNameIdentifierSegment".into(),
            TypedParser::new(SyntaxKind::Word, SyntaxKind::FunctionNameIdentifier)
                .to_matchable()
                .into(),
        ),
        // Maybe data types should be more restrictive?
        (
            "DatatypeIdentifierSegment".into(),
            SegmentGenerator::new(|_| {
                // Generate the anti template from the set of reserved keywords
                // TODO - this is a stopgap until we implement explicit data types
                let anti_template = format!("^({})$", "NOT");

                one_of(vec![
                    RegexParser::new("[A-Z_][A-Z0-9_]*", SyntaxKind::DataTypeIdentifier)
                        .anti_template(&anti_template)
                        .boxed(),
                    Ref::new("SingleIdentifierGrammar")
                        .exclude(Ref::new("NakedIdentifierSegment"))
                        .boxed(),
                ])
                .boxed()
            })
            .into(),
        ),
        // Ansi Intervals
        (
            "DatetimeUnitSegment".into(),
            SegmentGenerator::new(|dialect| {
                MultiStringParser::new(
                    dialect.sets("datetime_units").into_iter().map(Into::into).collect_vec(),
                    SyntaxKind::DatePart,
                )
                .boxed()
            })
            .into(),
        ),
        (
            "DatePartFunctionName".into(),
            SegmentGenerator::new(|dialect| {
                MultiStringParser::new(
                    dialect
                        .sets("date_part_function_name")
                        .into_iter()
                        .map(Into::into)
                        .collect::<Vec<_>>(),
                    SyntaxKind::FunctionNameIdentifier,
                )
                .boxed()
            })
            .into(),
        ),
        (
            "QuotedIdentifierSegment".into(),
            TypedParser::new(SyntaxKind::DoubleQuote, SyntaxKind::QuotedIdentifier)
                .to_matchable()
                .into(),
        ),
        (
            "QuotedLiteralSegment".into(),
            TypedParser::new(SyntaxKind::SingleQuote, SyntaxKind::QuotedLiteral)
                .to_matchable()
                .into(),
        ),
        (
            "SingleQuotedIdentifierSegment".into(),
            TypedParser::new(SyntaxKind::SingleQuote, SyntaxKind::QuotedIdentifier)
                .to_matchable()
                .into(),
        ),
        (
            "NumericLiteralSegment".into(),
            TypedParser::new(SyntaxKind::NumericLiteral, SyntaxKind::NumericLiteral)
                .to_matchable()
                .into(),
        ),
        // NullSegment is defined separately to the keyword, so we can give it a different
        // type
        (
            "NullLiteralSegment".into(),
            StringParser::new("null", SyntaxKind::NullLiteral).to_matchable().into(),
        ),
        (
            "NanLiteralSegment".into(),
            StringParser::new("nan", SyntaxKind::NullLiteral).to_matchable().into(),
        ),
        (
            "TrueSegment".into(),
            StringParser::new("true", SyntaxKind::BooleanLiteral).to_matchable().into(),
        ),
        (
            "FalseSegment".into(),
            StringParser::new("false", SyntaxKind::BooleanLiteral).to_matchable().into(),
        ),
        // We use a GRAMMAR here not a Segment. Otherwise, we get an unnecessary layer
        (
            "SingleIdentifierGrammar".into(),
            one_of(vec_of_erased![
                Ref::new("NakedIdentifierSegment"),
                Ref::new("QuotedIdentifierSegment")
            ])
            .config(|this| this.terminators = vec_of_erased![Ref::new("DotSegment")])
            .to_matchable()
            .into(),
        ),
        (
            "BooleanLiteralGrammar".into(),
            one_of(vec_of_erased![Ref::new("TrueSegment"), Ref::new("FalseSegment")])
                .to_matchable()
                .into(),
        ),
        // We specifically define a group of arithmetic operators to make it easier to
        // override this if some dialects have different available operators
        (
            "ArithmeticBinaryOperatorGrammar".into(),
            one_of(vec_of_erased![
                Ref::new("PlusSegment"),
                Ref::new("MinusSegment"),
                Ref::new("DivideSegment"),
                Ref::new("MultiplySegment"),
                Ref::new("ModuloSegment"),
                Ref::new("BitwiseAndSegment"),
                Ref::new("BitwiseOrSegment"),
                Ref::new("BitwiseXorSegment"),
                Ref::new("BitwiseLShiftSegment"),
                Ref::new("BitwiseRShiftSegment")
            ])
            .to_matchable()
            .into(),
        ),
        (
            "SignedSegmentGrammar".into(),
            one_of(vec_of_erased![Ref::new("PositiveSegment"), Ref::new("NegativeSegment")])
                .to_matchable()
                .into(),
        ),
        (
            "StringBinaryOperatorGrammar".into(),
            one_of(vec![Ref::new("ConcatSegment").boxed()]).to_matchable().into(),
        ),
        (
            "BooleanBinaryOperatorGrammar".into(),
            one_of(vec![
                Ref::new("AndOperatorGrammar").boxed(),
                Ref::new("OrOperatorGrammar").boxed(),
            ])
            .to_matchable()
            .into(),
        ),
        (
            "ComparisonOperatorGrammar".into(),
            one_of(vec_of_erased![
                Ref::new("EqualsSegment"),
                Ref::new("GreaterThanSegment"),
                Ref::new("LessThanSegment"),
                Ref::new("GreaterThanOrEqualToSegment"),
                Ref::new("LessThanOrEqualToSegment"),
                Ref::new("NotEqualToSegment"),
                Ref::new("LikeOperatorSegment"),
                Sequence::new(vec_of_erased![
                    Ref::keyword("IS"),
                    Ref::keyword("DISTINCT"),
                    Ref::keyword("FROM")
                ]),
                Sequence::new(vec_of_erased![
                    Ref::keyword("IS"),
                    Ref::keyword("NOT"),
                    Ref::keyword("DISTINCT"),
                    Ref::keyword("FROM")
                ])
            ])
            .to_matchable()
            .into(),
        ),
        // hookpoint for other dialects
        // e.g. EXASOL str to date cast with DATE '2021-01-01'
        // Give it a different type as needs to be single quotes and
        // should not be changed by rules (e.g. rule CV10)
        (
            "DateTimeLiteralGrammar".into(),
            Sequence::new(vec_of_erased![
                one_of(vec_of_erased![
                    Ref::keyword("DATE"),
                    Ref::keyword("TIME"),
                    Ref::keyword("TIMESTAMP"),
                    Ref::keyword("INTERVAL")
                ]),
                TypedParser::new(SyntaxKind::SingleQuote, SyntaxKind::DateConstructorLiteral,)
            ])
            .to_matchable()
            .into(),
        ),
        // Hookpoint for other dialects
        // e.g. INTO is optional in BIGQUERY
        (
            "MergeIntoLiteralGrammar".into(),
            Sequence::new(vec![Ref::keyword("MERGE").boxed(), Ref::keyword("INTO").boxed()])
                .to_matchable()
                .into(),
        ),
        (
            "LiteralGrammar".into(),
            one_of(vec_of_erased![
                Ref::new("QuotedLiteralSegment"),
                Ref::new("NumericLiteralSegment"),
                Ref::new("BooleanLiteralGrammar"),
                Ref::new("QualifiedNumericLiteralSegment"),
                // NB: Null is included in the literals, because it is a keyword which
                // can otherwise be easily mistaken for an identifier.
                Ref::new("NullLiteralSegment"),
                Ref::new("DateTimeLiteralGrammar"),
                Ref::new("ArrayLiteralSegment"),
                Ref::new("TypedArrayLiteralSegment"),
                Ref::new("ObjectLiteralSegment")
            ])
            .to_matchable()
            .into(),
        ),
        (
            "AndOperatorGrammar".into(),
            StringParser::new("AND", SyntaxKind::BinaryOperator).to_matchable().into(),
        ),
        (
            "OrOperatorGrammar".into(),
            StringParser::new("OR", SyntaxKind::BinaryOperator).to_matchable().into(),
        ),
        (
            "NotOperatorGrammar".into(),
            StringParser::new("NOT", SyntaxKind::Keyword).to_matchable().into(),
        ),
        (
            // This is a placeholder for other dialects.
            "PreTableFunctionKeywordsGrammar".into(),
            Nothing::new().to_matchable().into(),
        ),
        (
            "BinaryOperatorGrammar".into(),
            one_of(vec_of_erased![
                Ref::new("ArithmeticBinaryOperatorGrammar"),
                Ref::new("StringBinaryOperatorGrammar"),
                Ref::new("BooleanBinaryOperatorGrammar"),
                Ref::new("ComparisonOperatorGrammar")
            ])
            .to_matchable()
            .into(),
        ),
        // This pattern is used in a lot of places.
        // Defined here to avoid repetition.
        (
            "BracketedColumnReferenceListGrammar".into(),
            Bracketed::new(vec![
                Delimited::new(vec![Ref::new("ColumnReferenceSegment").boxed()]).boxed(),
            ])
            .to_matchable()
            .into(),
        ),
        (
            "OrReplaceGrammar".into(),
            Sequence::new(vec![Ref::keyword("OR").boxed(), Ref::keyword("REPLACE").boxed()])
                .to_matchable()
                .into(),
        ),
        (
            "TemporaryTransientGrammar".into(),
            one_of(vec![Ref::keyword("TRANSIENT").boxed(), Ref::new("TemporaryGrammar").boxed()])
                .to_matchable()
                .into(),
        ),
        (
            "TemporaryGrammar".into(),
            one_of(vec![Ref::keyword("TEMP").boxed(), Ref::keyword("TEMPORARY").boxed()])
                .to_matchable()
                .into(),
        ),
        (
            "IfExistsGrammar".into(),
            Sequence::new(vec![Ref::keyword("IF").boxed(), Ref::keyword("EXISTS").boxed()])
                .to_matchable()
                .into(),
        ),
        (
            "IfNotExistsGrammar".into(),
            Sequence::new(vec_of_erased![
                Ref::keyword("IF"),
                Ref::keyword("NOT"),
                Ref::keyword("EXISTS")
            ])
            .to_matchable()
            .into(),
        ),
        (
            "LikeGrammar".into(),
            one_of(vec_of_erased![
                Ref::keyword("LIKE"),
                Ref::keyword("RLIKE"),
                Ref::keyword("ILIKE")
            ])
            .to_matchable()
            .into(),
        ),
        (
            "UnionGrammar".into(),
            Sequence::new(vec_of_erased![
                Ref::keyword("UNION"),
                one_of(vec_of_erased![Ref::keyword("DISTINCT"), Ref::keyword("ALL")])
                    .config(|this| this.optional())
            ])
            .to_matchable()
            .into(),
        ),
        (
            "IsClauseGrammar".into(),
            one_of(vec![
                Ref::new("NullLiteralSegment").boxed(),
                Ref::new("NanLiteralSegment").boxed(),
                Ref::new("BooleanLiteralGrammar").boxed(),
            ])
            .to_matchable()
            .into(),
        ),
        (
            "InOperatorGrammar".into(),
            Sequence::new(vec_of_erased![
                Ref::keyword("NOT").optional(),
                Ref::keyword("IN"),
                one_of(vec_of_erased![
                    Bracketed::new(vec_of_erased![one_of(vec_of_erased![
                        Delimited::new(vec_of_erased![Ref::new("Expression_A_Grammar"),]),
                        Ref::new("SelectableGrammar"),
                    ])])
                    .config(|this| this.parse_mode(ParseMode::Greedy)),
                    Ref::new("FunctionSegment"), // E.g. UNNEST()
                ]),
            ])
            .to_matchable()
            .into(),
        ),
        (
            "SelectClauseTerminatorGrammar".into(),
            one_of(vec_of_erased![
                Ref::keyword("FROM"),
                Ref::keyword("WHERE"),
                Sequence::new(vec_of_erased![Ref::keyword("ORDER"), Ref::keyword("BY")]),
                Ref::keyword("LIMIT"),
                Ref::keyword("OVERLAPS"),
                Ref::new("SetOperatorSegment"),
                Ref::keyword("FETCH"),
            ])
            .to_matchable()
            .into(),
        ),
        // Define these as grammars to allow child dialects to enable them (since they are
        // non-standard keywords)
        ("IsNullGrammar".into(), Nothing::new().to_matchable().into()),
        ("NotNullGrammar".into(), Nothing::new().to_matchable().into()),
        ("CollateGrammar".into(), Nothing::new().to_matchable().into()),
        (
            "FromClauseTerminatorGrammar".into(),
            one_of(vec![
                Ref::keyword("WHERE").boxed(),
                Ref::keyword("LIMIT").boxed(),
                Sequence::new(vec![Ref::keyword("GROUP").boxed(), Ref::keyword("BY").boxed()])
                    .boxed(),
                Sequence::new(vec![Ref::keyword("ORDER").boxed(), Ref::keyword("BY").boxed()])
                    .boxed(),
                Ref::keyword("HAVING").boxed(),
                Ref::keyword("QUALIFY").boxed(),
                Ref::keyword("WINDOW").boxed(),
                Ref::new("SetOperatorSegment").boxed(),
                Ref::new("WithNoSchemaBindingClauseSegment").boxed(),
                Ref::new("WithDataClauseSegment").boxed(),
                Ref::keyword("FETCH").boxed(),
            ])
            .to_matchable()
            .into(),
        ),
        (
            "WhereClauseTerminatorGrammar".into(),
            one_of(vec![
                Ref::keyword("LIMIT").boxed(),
                Sequence::new(vec![Ref::keyword("GROUP").boxed(), Ref::keyword("BY").boxed()])
                    .boxed(),
                Sequence::new(vec![Ref::keyword("ORDER").boxed(), Ref::keyword("BY").boxed()])
                    .boxed(),
                Ref::keyword("HAVING").boxed(),
                Ref::keyword("QUALIFY").boxed(),
                Ref::keyword("WINDOW").boxed(),
                Ref::keyword("OVERLAPS").boxed(),
                Ref::keyword("FETCH").boxed(),
            ])
            .to_matchable()
            .into(),
        ),
        (
            "GroupByClauseTerminatorGrammar".into(),
            one_of(vec![
                Sequence::new(vec![Ref::keyword("ORDER").boxed(), Ref::keyword("BY").boxed()])
                    .boxed(),
                Ref::keyword("LIMIT").boxed(),
                Ref::keyword("HAVING").boxed(),
                Ref::keyword("QUALIFY").boxed(),
                Ref::keyword("WINDOW").boxed(),
                Ref::keyword("FETCH").boxed(),
            ])
            .to_matchable()
            .into(),
        ),
        (
            "HavingClauseTerminatorGrammar".into(),
            one_of(vec![
                Sequence::new(vec![Ref::keyword("ORDER").boxed(), Ref::keyword("BY").boxed()])
                    .boxed(),
                Ref::keyword("LIMIT").boxed(),
                Ref::keyword("QUALIFY").boxed(),
                Ref::keyword("WINDOW").boxed(),
                Ref::keyword("FETCH").boxed(),
            ])
            .to_matchable()
            .into(),
        ),
        (
            "OrderByClauseTerminators".into(),
            one_of(vec![
                Ref::keyword("LIMIT").boxed(),
                Ref::keyword("HAVING").boxed(),
                Ref::keyword("QUALIFY").boxed(),
                Ref::keyword("WINDOW").boxed(),
                Ref::new("FrameClauseUnitGrammar").boxed(),
                Ref::keyword("SEPARATOR").boxed(),
                Ref::keyword("FETCH").boxed(),
            ])
            .to_matchable()
            .into(),
        ),
        (
            "PrimaryKeyGrammar".into(),
            Sequence::new(vec![Ref::keyword("PRIMARY").boxed(), Ref::keyword("KEY").boxed()])
                .to_matchable()
                .into(),
        ),
        (
            "ForeignKeyGrammar".into(),
            Sequence::new(vec![Ref::keyword("FOREIGN").boxed(), Ref::keyword("KEY").boxed()])
                .to_matchable()
                .into(),
        ),
        (
            "UniqueKeyGrammar".into(),
            Sequence::new(vec![Ref::keyword("UNIQUE").boxed()]).to_matchable().into(),
        ),
        // Odd syntax, but prevents eager parameters being confused for data types
        (
            "FunctionParameterGrammar".into(),
            one_of(vec![
                Sequence::new(vec![
                    Ref::new("ParameterNameSegment").optional().boxed(),
                    one_of(vec![
                        Sequence::new(vec![
                            Ref::keyword("ANY").boxed(),
                            Ref::keyword("TYPE").boxed(),
                        ])
                        .boxed(),
                        Ref::new("DatatypeSegment").boxed(),
                    ])
                    .boxed(),
                ])
                .boxed(),
                one_of(vec![
                    Sequence::new(vec![Ref::keyword("ANY").boxed(), Ref::keyword("TYPE").boxed()])
                        .boxed(),
                    Ref::new("DatatypeSegment").boxed(),
                ])
                .boxed(),
            ])
            .to_matchable()
            .into(),
        ),
        (
            "AutoIncrementGrammar".into(),
            Sequence::new(vec![Ref::keyword("AUTO_INCREMENT").boxed()]).to_matchable().into(),
        ),
        // Base Expression element is the right thing to reference for everything
        // which functions as an expression, but could include literals.
        (
            "BaseExpressionElementGrammar".into(),
            one_of(vec![
                Ref::new("LiteralGrammar").boxed(),
                Ref::new("BareFunctionSegment").boxed(),
                Ref::new("IntervalExpressionSegment").boxed(),
                Ref::new("FunctionSegment").boxed(),
                Ref::new("ColumnReferenceSegment").boxed(),
                Ref::new("ExpressionSegment").boxed(),
                Sequence::new(vec![
                    Ref::new("DatatypeSegment").boxed(),
                    Ref::new("LiteralGrammar").boxed(),
                ])
                .boxed(),
            ])
            .config(|this| {
                // These terminators allow better performance by giving a signal
                // of a likely complete match if they come after a match. For
                // example "123," only needs to match against the LiteralGrammar
                // and because a comma follows, never be matched against
                // ExpressionSegment or FunctionSegment, which are both much
                // more complicated.

                this.terminators = vec_of_erased![
                    Ref::new("CommaSegment"),
                    Ref::keyword("AS"),
                    // TODO: We can almost certainly add a few more here.
                ];
            })
            .to_matchable()
            .into(),
        ),
        (
            "FilterClauseGrammar".into(),
            Sequence::new(vec![
                Ref::keyword("FILTER").boxed(),
                Bracketed::new(vec![
                    Sequence::new(vec![
                        Ref::keyword("WHERE").boxed(),
                        Ref::new("ExpressionSegment").boxed(),
                    ])
                    .boxed(),
                ])
                .boxed(),
            ])
            .to_matchable()
            .into(),
        ),
        (
            "IgnoreRespectNullsGrammar".into(),
            Sequence::new(vec![
                one_of(vec![Ref::keyword("IGNORE").boxed(), Ref::keyword("RESPECT").boxed()])
                    .boxed(),
                Ref::keyword("NULLS").boxed(),
            ])
            .to_matchable()
            .into(),
        ),
        (
            "FrameClauseUnitGrammar".into(),
            one_of(vec![Ref::keyword("ROWS").boxed(), Ref::keyword("RANGE").boxed()])
                .to_matchable()
                .into(),
        ),
        (
            "JoinTypeKeywordsGrammar".into(),
            one_of(vec![
                Ref::keyword("CROSS").boxed(),
                Ref::keyword("INNER").boxed(),
                Sequence::new(vec![
                    one_of(vec![
                        Ref::keyword("FULL").boxed(),
                        Ref::keyword("LEFT").boxed(),
                        Ref::keyword("RIGHT").boxed(),
                    ])
                    .boxed(),
                    Ref::keyword("OUTER").optional().boxed(),
                ])
                .boxed(),
            ])
            .config(|this| this.optional())
            .to_matchable()
            .into(),
        ),
        (
            // It's as a sequence to allow to parametrize that in Postgres dialect with LATERAL
            "JoinKeywordsGrammar".into(),
            Sequence::new(vec![Ref::keyword("JOIN").boxed()]).to_matchable().into(),
        ),
        (
            // NATURAL joins are not supported in all dialects (e.g. not in Bigquery
            // or T-SQL). So define here to allow override with Nothing() for those.
            "NaturalJoinKeywordsGrammar".into(),
            Sequence::new(vec![
                Ref::keyword("NATURAL").boxed(),
                one_of(vec![
                    // Note: NATURAL joins do not support CROSS joins
                    Ref::keyword("INNER").boxed(),
                    Sequence::new(vec![
                        one_of(vec![
                            Ref::keyword("LEFT").boxed(),
                            Ref::keyword("RIGHT").boxed(),
                            Ref::keyword("FULL").boxed(),
                        ])
                        .boxed(),
                        Ref::keyword("OUTER").optional().boxed(),
                    ])
                    .config(|this| this.optional())
                    .boxed(),
                ])
                .config(|this| this.optional())
                .boxed(),
            ])
            .to_matchable()
            .into(),
        ),
        // This can be overwritten by dialects
        ("ExtendedNaturalJoinKeywordsGrammar".into(), Nothing::new().to_matchable().into()),
        ("NestedJoinGrammar".into(), Nothing::new().to_matchable().into()),
        (
            "ReferentialActionGrammar".into(),
            one_of(vec![
                Ref::keyword("RESTRICT").boxed(),
                Ref::keyword("CASCADE").boxed(),
                Sequence::new(vec![Ref::keyword("SET").boxed(), Ref::keyword("NULL").boxed()])
                    .boxed(),
                Sequence::new(vec![Ref::keyword("NO").boxed(), Ref::keyword("ACTION").boxed()])
                    .boxed(),
                Sequence::new(vec![Ref::keyword("SET").boxed(), Ref::keyword("DEFAULT").boxed()])
                    .boxed(),
            ])
            .to_matchable()
            .into(),
        ),
        (
            "DropBehaviorGrammar".into(),
            one_of(vec![Ref::keyword("RESTRICT").boxed(), Ref::keyword("CASCADE").boxed()])
                .config(|this| this.optional())
                .to_matchable()
                .into(),
        ),
        (
            "ColumnConstraintDefaultGrammar".into(),
            one_of(vec![
                Ref::new("ShorthandCastSegment").boxed(),
                Ref::new("LiteralGrammar").boxed(),
                Ref::new("FunctionSegment").boxed(),
                Ref::new("BareFunctionSegment").boxed(),
            ])
            .to_matchable()
            .into(),
        ),
        (
            "ReferenceDefinitionGrammar".into(),
            Sequence::new(vec_of_erased![
                Ref::keyword("REFERENCES"),
                Ref::new("TableReferenceSegment"),
                // Foreign columns making up FOREIGN KEY constraint
                Ref::new("BracketedColumnReferenceListGrammar").optional(),
                Sequence::new(vec_of_erased![
                    Ref::keyword("MATCH"),
                    one_of(vec_of_erased![
                        Ref::keyword("FULL"),
                        Ref::keyword("PARTIAL"),
                        Ref::keyword("SIMPLE")
                    ])
                ])
                .config(|this| this.optional()),
                AnyNumberOf::new(vec_of_erased![
                    // ON DELETE clause, e.g. ON DELETE NO ACTION
                    Sequence::new(vec_of_erased![
                        Ref::keyword("ON"),
                        Ref::keyword("DELETE"),
                        Ref::new("ReferentialActionGrammar")
                    ]),
                    Sequence::new(vec_of_erased![
                        Ref::keyword("ON"),
                        Ref::keyword("UPDATE"),
                        Ref::new("ReferentialActionGrammar")
                    ])
                ])
            ])
            .to_matchable()
            .into(),
        ),
        (
            "TrimParametersGrammar".into(),
            one_of(vec![
                Ref::keyword("BOTH").boxed(),
                Ref::keyword("LEADING").boxed(),
                Ref::keyword("TRAILING").boxed(),
            ])
            .to_matchable()
            .into(),
        ),
        (
            "DefaultValuesGrammar".into(),
            Sequence::new(vec![Ref::keyword("DEFAULT").boxed(), Ref::keyword("VALUES").boxed()])
                .to_matchable()
                .into(),
        ),
        (
            "ObjectReferenceDelimiterGrammar".into(),
            one_of(vec![
                Ref::new("DotSegment").boxed(),
                // NOTE: The double dot syntax allows for default values.
                Sequence::new(vec![Ref::new("DotSegment").boxed(), Ref::new("DotSegment").boxed()])
                    .boxed(),
            ])
            .to_matchable()
            .into(),
        ),
        (
            "ObjectReferenceTerminatorGrammar".into(),
            one_of(vec![
                Ref::keyword("ON").boxed(),
                Ref::keyword("AS").boxed(),
                Ref::keyword("USING").boxed(),
                Ref::new("CommaSegment").boxed(),
                Ref::new("CastOperatorSegment").boxed(),
                Ref::new("StartSquareBracketSegment").boxed(),
                Ref::new("StartBracketSegment").boxed(),
                Ref::new("BinaryOperatorGrammar").boxed(),
                Ref::new("ColonSegment").boxed(),
                Ref::new("DelimiterGrammar").boxed(),
                Ref::new("JoinLikeClauseGrammar").boxed(),
                Bracketed::new(vec![]).boxed(),
            ])
            .to_matchable()
            .into(),
        ),
        (
            "AlterTableOptionsGrammar".into(),
            one_of(vec_of_erased![
                // Table options
                Sequence::new(vec_of_erased![
                    Ref::new("ParameterNameSegment"),
                    Ref::new("EqualsSegment").optional(),
                    one_of(vec_of_erased![
                        Ref::new("LiteralGrammar"),
                        Ref::new("NakedIdentifierSegment")
                    ])
                ]),
                // Add things
                Sequence::new(vec_of_erased![
                    one_of(vec_of_erased![Ref::keyword("ADD"), Ref::keyword("MODIFY")]),
                    Ref::keyword("COLUMN").optional(),
                    Ref::new("ColumnDefinitionSegment"),
                    one_of(vec_of_erased![Sequence::new(vec_of_erased![one_of(vec_of_erased![
                        Ref::keyword("FIRST"),
                        Ref::keyword("AFTER"),
                        Ref::new("ColumnReferenceSegment"),
                        // Bracketed Version of the same
                        Ref::new("BracketedColumnReferenceListGrammar")
                    ])])])
                    .config(|this| this.optional())
                ]),
                // Rename
                Sequence::new(vec_of_erased![
                    Ref::keyword("RENAME"),
                    one_of(vec_of_erased![Ref::keyword("AS"), Ref::keyword("TO")])
                        .config(|this| this.optional()),
                    Ref::new("TableReferenceSegment")
                ])
            ])
            .to_matchable()
            .into(),
        ),
    ]);

    ansi_dialect.add([
        (
            "FileSegment".into(),
            NodeMatcher::new(
                SyntaxKind::File,
                Delimited::new(vec![Ref::new("StatementSegment").boxed()])
                    .config(|this| {
                        this.allow_trailing();
                        this.delimiter(
                            AnyNumberOf::new(vec![Ref::new("DelimiterGrammar").boxed()])
                                .config(|config| config.min_times(1)),
                        );
                    })
                    .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "ColumnReferenceSegment".into(),
            NodeMatcher::new(
                SyntaxKind::ColumnReference,
                Delimited::new(vec![Ref::new("SingleIdentifierGrammar").boxed()])
                    .config(|this| this.delimiter(Ref::new("ObjectReferenceDelimiterGrammar")))
                    .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "ExpressionSegment".into(),
            NodeMatcher::new(
                SyntaxKind::Expression,
                Ref::new("Expression_A_Grammar").to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "WildcardIdentifierSegment".into(),
            NodeMatcher::new(
                SyntaxKind::WildcardIdentifier,
                Sequence::new(vec![
                    AnyNumberOf::new(vec![
                        Sequence::new(vec![
                            Ref::new("SingleIdentifierGrammar").boxed(),
                            Ref::new("ObjectReferenceDelimiterGrammar").boxed(),
                        ])
                        .boxed(),
                    ])
                    .boxed(),
                    Ref::new("StarSegment").boxed(),
                ])
                .allow_gaps(false)
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "NamedWindowExpressionSegment".into(),
            NodeMatcher::new(
                SyntaxKind::NamedWindowExpression,
                Sequence::new(vec_of_erased![
                    Ref::new("SingleIdentifierGrammar"),
                    Ref::keyword("AS"),
                    one_of(vec_of_erased![
                        Ref::new("SingleIdentifierGrammar"),
                        Bracketed::new(vec_of_erased![Ref::new("WindowSpecificationSegment")])
                            .config(|this| this.parse_mode(ParseMode::Greedy)),
                    ]),
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "FunctionSegment".into(),
            NodeMatcher::new(
                SyntaxKind::Function,
                one_of(vec_of_erased![
                    Sequence::new(vec_of_erased![Sequence::new(vec_of_erased![
                        Ref::new("DatePartFunctionNameSegment"),
                        Bracketed::new(vec_of_erased![Delimited::new(vec_of_erased![
                            Ref::new("DatetimeUnitSegment"),
                            Ref::new("FunctionContentsGrammar").optional()
                        ])])
                        .config(|this| this.parse_mode(ParseMode::Greedy))
                    ])]),
                    Sequence::new(vec_of_erased![
                        Sequence::new(vec_of_erased![
                            Ref::new("FunctionNameSegment").exclude(one_of(vec_of_erased![
                                Ref::new("DatePartFunctionNameSegment"),
                                Ref::new("ValuesClauseSegment")
                            ])),
                            Bracketed::new(vec_of_erased![
                                Ref::new("FunctionContentsGrammar").optional()
                            ])
                            .config(|this| this.parse_mode(ParseMode::Greedy))
                        ]),
                        Ref::new("PostFunctionGrammar").optional()
                    ])
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "HavingClauseSegment".into(),
            NodeMatcher::new(
                SyntaxKind::HavingClause,
                Sequence::new(vec_of_erased![
                    Ref::keyword("HAVING"),
                    MetaSegment::implicit_indent(),
                    optionally_bracketed(vec_of_erased![Ref::new("ExpressionSegment")]),
                    MetaSegment::dedent()
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "PathSegment".into(),
            NodeMatcher::new(
                SyntaxKind::PathSegment,
                one_of(vec_of_erased![
                    Sequence::new(vec_of_erased![
                        Ref::new("SlashSegment"),
                        Delimited::new(vec_of_erased![TypedParser::new(
                            SyntaxKind::Word,
                            SyntaxKind::PathSegment,
                        )])
                        .config(|this| {
                            this.allow_gaps = false;
                            this.delimiter(Ref::new("SlashSegment"));
                        }),
                    ]),
                    Ref::new("QuotedLiteralSegment"),
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "LimitClauseSegment".into(),
            NodeMatcher::new(
                SyntaxKind::LimitClause,
                Sequence::new(vec_of_erased![
                    Ref::keyword("LIMIT"),
                    MetaSegment::indent(),
                    optionally_bracketed(vec_of_erased![one_of(vec_of_erased![
                        Ref::new("NumericLiteralSegment"),
                        Ref::new("ExpressionSegment"),
                        Ref::keyword("ALL"),
                    ])]),
                    one_of(vec_of_erased![
                        Sequence::new(vec_of_erased![
                            Ref::keyword("OFFSET"),
                            one_of(vec_of_erased![
                                Ref::new("NumericLiteralSegment"),
                                Ref::new("ExpressionSegment"),
                            ]),
                        ]),
                        Sequence::new(vec_of_erased![
                            Ref::new("CommaSegment"),
                            Ref::new("NumericLiteralSegment"),
                        ]),
                    ])
                    .config(|this| this.optional()),
                    MetaSegment::dedent()
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "CubeRollupClauseSegment".into(),
            NodeMatcher::new(
                SyntaxKind::CubeRollupClause,
                Sequence::new(vec_of_erased![
                    one_of(vec_of_erased![
                        Ref::new("CubeFunctionNameSegment"),
                        Ref::new("RollupFunctionNameSegment"),
                    ]),
                    Bracketed::new(vec_of_erased![Ref::new("GroupingExpressionList")]),
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "RollupFunctionNameSegment".into(),
            NodeMatcher::new(
                SyntaxKind::FunctionName,
                StringParser::new("ROLLUP", SyntaxKind::FunctionNameIdentifier).boxed(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "CubeFunctionNameSegment".into(),
            NodeMatcher::new(
                SyntaxKind::FunctionName,
                StringParser::new("CUBE", SyntaxKind::FunctionNameIdentifier).boxed(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "GroupingSetsClauseSegment".into(),
            NodeMatcher::new(
                SyntaxKind::GroupingSetsClause,
                Sequence::new(vec_of_erased![
                    Ref::keyword("GROUPING"),
                    Ref::keyword("SETS"),
                    Bracketed::new(vec_of_erased![Delimited::new(vec_of_erased![
                        Ref::new("CubeRollupClauseSegment"),
                        Ref::new("GroupingExpressionList"),
                    ])]),
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "GroupingExpressionList".into(),
            NodeMatcher::new(
                SyntaxKind::GroupingExpressionList,
                Sequence::new(vec_of_erased![
                    MetaSegment::indent(),
                    Delimited::new(vec_of_erased![
                        one_of(vec_of_erased![
                            Ref::new("ColumnReferenceSegment"),
                            Ref::new("NumericLiteralSegment"),
                            Ref::new("ExpressionSegment"),
                            Bracketed::new(vec_of_erased![]),
                        ]),
                        Ref::new("GroupByClauseTerminatorGrammar"),
                    ]),
                    MetaSegment::dedent(),
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "SetClauseSegment".into(),
            NodeMatcher::new(
                SyntaxKind::SetClause,
                Sequence::new(vec_of_erased![
                    Ref::new("ColumnReferenceSegment"),
                    Ref::new("EqualsSegment"),
                    one_of(vec_of_erased![
                        Ref::new("LiteralGrammar"),
                        Ref::new("BareFunctionSegment"),
                        Ref::new("FunctionSegment"),
                        Ref::new("ColumnReferenceSegment"),
                        Ref::new("ExpressionSegment"),
                        Ref::new("ValuesClauseSegment"),
                        Ref::keyword("DEFAULT"),
                    ]),
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "FetchClauseSegment".into(),
            NodeMatcher::new(
                SyntaxKind::FetchClause,
                Sequence::new(vec_of_erased![
                    Ref::keyword("FETCH"),
                    one_of(vec_of_erased![Ref::keyword("FIRST"), Ref::keyword("NEXT")]),
                    Ref::new("NumericLiteralSegment").optional(),
                    one_of(vec_of_erased![Ref::keyword("ROW"), Ref::keyword("ROWS")]),
                    Ref::keyword("ONLY"),
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "FunctionDefinitionGrammar".into(),
            NodeMatcher::new(
                SyntaxKind::FunctionDefinition,
                Sequence::new(vec_of_erased![
                    Ref::keyword("AS"),
                    Ref::new("QuotedLiteralSegment"),
                    Sequence::new(vec_of_erased![
                        Ref::keyword("LANGUAGE"),
                        Ref::new("NakedIdentifierSegment")
                    ])
                    .config(|this| this.optional()),
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "AlterSequenceOptionsSegment".into(),
            NodeMatcher::new(
                SyntaxKind::AlterSequenceOptionsSegment,
                one_of(vec_of_erased![
                    Sequence::new(vec_of_erased![
                        Ref::keyword("INCREMENT"),
                        Ref::keyword("BY"),
                        Ref::new("NumericLiteralSegment")
                    ]),
                    one_of(vec_of_erased![
                        Sequence::new(vec_of_erased![
                            Ref::keyword("MINVALUE"),
                            Ref::new("NumericLiteralSegment")
                        ]),
                        Sequence::new(vec_of_erased![Ref::keyword("NO"), Ref::keyword("MINVALUE")])
                    ]),
                    one_of(vec_of_erased![
                        Sequence::new(vec_of_erased![
                            Ref::keyword("MAXVALUE"),
                            Ref::new("NumericLiteralSegment")
                        ]),
                        Sequence::new(vec_of_erased![Ref::keyword("NO"), Ref::keyword("MAXVALUE")])
                    ]),
                    one_of(vec_of_erased![
                        Sequence::new(vec_of_erased![
                            Ref::keyword("CACHE"),
                            Ref::new("NumericLiteralSegment")
                        ]),
                        Ref::keyword("NOCACHE")
                    ]),
                    one_of(vec_of_erased![Ref::keyword("CYCLE"), Ref::keyword("NOCYCLE")]),
                    one_of(vec_of_erased![Ref::keyword("ORDER"), Ref::keyword("NOORDER")])
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "RoleReferenceSegment".into(),
            NodeMatcher::new(
                SyntaxKind::RoleReference,
                Ref::new("SingleIdentifierGrammar").to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "TablespaceReferenceSegment".into(),
            NodeMatcher::new(
                SyntaxKind::TablespaceReference,
                Delimited::new(vec![Ref::new("SingleIdentifierGrammar").boxed()])
                    .config(|this| {
                        this.delimiter(Ref::new("ObjectReferenceDelimiterGrammar"));
                        this.disallow_gaps();
                        this.terminators =
                            vec_of_erased![Ref::new("ObjectReferenceTerminatorGrammar")];
                    })
                    .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "ExtensionReferenceSegment".into(),
            NodeMatcher::new(
                SyntaxKind::ExtensionReference,
                Delimited::new(vec![Ref::new("SingleIdentifierGrammar").boxed()])
                    .config(|this| {
                        this.delimiter(Ref::new("ObjectReferenceDelimiterGrammar"));
                        this.disallow_gaps();
                        this.terminators =
                            vec_of_erased![Ref::new("ObjectReferenceTerminatorGrammar")];
                    })
                    .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "TagReferenceSegment".into(),
            NodeMatcher::new(
                SyntaxKind::TagReference,
                Delimited::new(vec![Ref::new("SingleIdentifierGrammar").boxed()])
                    .config(|this| {
                        this.delimiter(Ref::new("ObjectReferenceDelimiterGrammar"));
                        this.disallow_gaps();
                        this.terminators =
                            vec_of_erased![Ref::new("ObjectReferenceTerminatorGrammar")];
                    })
                    .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "ColumnDefinitionSegment".into(),
            NodeMatcher::new(
                SyntaxKind::ColumnDefinition,
                Sequence::new(vec_of_erased![
                    Ref::new("SingleIdentifierGrammar"), // Column name
                    Ref::new("DatatypeSegment"),         // Column type,
                    Bracketed::new(vec_of_erased![Anything::new()]).config(|this| this.optional()),
                    AnyNumberOf::new(vec_of_erased![Ref::new("ColumnConstraintSegment")])
                        .config(|this| this.optional())
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "ColumnConstraintSegment".into(),
            NodeMatcher::new(
                SyntaxKind::ColumnConstraintSegment,
                Sequence::new(vec_of_erased![
                    Sequence::new(vec_of_erased![
                        Ref::keyword("CONSTRAINT"),
                        Ref::new("ObjectReferenceSegment"), // Constraint name
                    ])
                    .config(|this| this.optional()),
                    one_of(vec_of_erased![
                        Sequence::new(vec_of_erased![
                            Ref::keyword("NOT").optional(),
                            Ref::keyword("NULL"),
                        ]),
                        Sequence::new(vec_of_erased![
                            Ref::keyword("CHECK"),
                            Bracketed::new(vec_of_erased![Ref::new("ExpressionSegment")]),
                        ]),
                        Sequence::new(vec_of_erased![
                            Ref::keyword("DEFAULT"),
                            Ref::new("ColumnConstraintDefaultGrammar"),
                        ]),
                        Ref::new("PrimaryKeyGrammar"),
                        Ref::new("UniqueKeyGrammar"), // UNIQUE
                        Ref::new("AutoIncrementGrammar"),
                        Ref::new("ReferenceDefinitionGrammar"), /* REFERENCES reftable [ (
                                                                 * refcolumn) ] */
                        Ref::new("CommentClauseSegment"),
                        Sequence::new(vec_of_erased![
                            Ref::keyword("COLLATE"),
                            Ref::new("CollationReferenceSegment"),
                        ]), // COLLATE
                    ]),
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "CommentClauseSegment".into(),
            NodeMatcher::new(
                SyntaxKind::CommentClause,
                Sequence::new(vec_of_erased![
                    Ref::keyword("COMMENT"),
                    Ref::new("QuotedLiteralSegment")
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "TableEndClauseSegment".into(),
            NodeMatcher::new(SyntaxKind::TableEndClause, Nothing::new().to_matchable())
                .to_matchable()
                .into(),
        ),
        (
            "MergeMatchSegment".into(),
            NodeMatcher::new(
                SyntaxKind::MergeMatch,
                AnyNumberOf::new(vec_of_erased![
                    Ref::new("MergeMatchedClauseSegment"),
                    Ref::new("MergeNotMatchedClauseSegment")
                ])
                .config(|this| this.min_times(1))
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "MergeMatchedClauseSegment".into(),
            NodeMatcher::new(
                SyntaxKind::MergeWhenMatchedClause,
                Sequence::new(vec_of_erased![
                    Ref::keyword("WHEN"),
                    Ref::keyword("MATCHED"),
                    Sequence::new(vec_of_erased![
                        Ref::keyword("AND"),
                        Ref::new("ExpressionSegment")
                    ])
                    .config(|this| this.optional()),
                    Ref::keyword("THEN"),
                    MetaSegment::indent(),
                    one_of(vec_of_erased![
                        Ref::new("MergeUpdateClauseSegment"),
                        Ref::new("MergeDeleteClauseSegment")
                    ]),
                    MetaSegment::dedent()
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "MergeNotMatchedClauseSegment".into(),
            NodeMatcher::new(
                SyntaxKind::MergeWhenNotMatchedClause,
                Sequence::new(vec_of_erased![
                    Ref::keyword("WHEN"),
                    Ref::keyword("NOT"),
                    Ref::keyword("MATCHED"),
                    Sequence::new(vec_of_erased![
                        Ref::keyword("AND"),
                        Ref::new("ExpressionSegment")
                    ])
                    .config(|this| this.optional()),
                    Ref::keyword("THEN"),
                    MetaSegment::indent(),
                    Ref::new("MergeInsertClauseSegment"),
                    MetaSegment::dedent(),
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "MergeInsertClauseSegment".into(),
            NodeMatcher::new(
                SyntaxKind::MergeInsertClause,
                Sequence::new(vec_of_erased![
                    Ref::keyword("INSERT"),
                    MetaSegment::indent(),
                    Ref::new("BracketedColumnReferenceListGrammar").optional(),
                    MetaSegment::dedent(),
                    Ref::new("ValuesClauseSegment").optional()
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "MergeUpdateClauseSegment".into(),
            NodeMatcher::new(
                SyntaxKind::MergeUpdateClause,
                Sequence::new(vec_of_erased![
                    Ref::keyword("UPDATE"),
                    MetaSegment::indent(),
                    Ref::new("SetClauseListSegment"),
                    MetaSegment::dedent(),
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "MergeDeleteClauseSegment".into(),
            NodeMatcher::new(SyntaxKind::MergeDeleteClause, Ref::keyword("DELETE").to_matchable())
                .to_matchable()
                .into(),
        ),
        (
            "SetClauseListSegment".into(),
            NodeMatcher::new(
                SyntaxKind::SetClauseList,
                Sequence::new(vec_of_erased![
                    Ref::keyword("SET"),
                    MetaSegment::indent(),
                    Ref::new("SetClauseSegment"),
                    AnyNumberOf::new(vec_of_erased![
                        Ref::new("CommaSegment"),
                        Ref::new("SetClauseSegment"),
                    ]),
                    MetaSegment::dedent(),
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "TableReferenceSegment".into(),
            NodeMatcher::new(
                SyntaxKind::TableReference,
                ansi_dialect.grammar("ObjectReferenceSegment").match_grammar().unwrap().clone(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "SchemaReferenceSegment".into(),
            NodeMatcher::new(
                SyntaxKind::TableReference,
                Ref::new("ObjectReferenceSegment").to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "SingleIdentifierListSegment".into(),
            NodeMatcher::new(
                SyntaxKind::IdentifierList,
                Delimited::new(vec_of_erased![Ref::new("SingleIdentifierGrammar")])
                    .config(|this| this.optional())
                    .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "GroupByClauseSegment".into(),
            NodeMatcher::new(
                SyntaxKind::GroupbyClause,
                Sequence::new(vec_of_erased![
                    Ref::keyword("GROUP"),
                    Ref::keyword("BY"),
                    one_of(vec_of_erased![
                        Ref::new("CubeRollupClauseSegment"),
                        Sequence::new(vec_of_erased![
                            MetaSegment::indent(),
                            Delimited::new(vec_of_erased![one_of(vec_of_erased![
                                Ref::new("ColumnReferenceSegment"),
                                Ref::new("NumericLiteralSegment"),
                                Ref::new("ExpressionSegment"),
                            ])])
                            .config(|this| {
                                this.terminators =
                                    vec![Ref::new("GroupByClauseTerminatorGrammar").boxed()];
                            }),
                            MetaSegment::dedent()
                        ])
                    ])
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "FrameClauseSegment".into(),
            NodeMatcher::new(
                SyntaxKind::FrameClause,
                Sequence::new(vec_of_erased![
                    Ref::new("FrameClauseUnitGrammar"),
                    one_of(vec_of_erased![
                        frame_extent(),
                        Sequence::new(vec_of_erased![
                            Ref::keyword("BETWEEN"),
                            frame_extent(),
                            Ref::keyword("AND"),
                            frame_extent(),
                        ])
                    ])
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "WithCompoundStatementSegment".into(),
            NodeMatcher::new(
                SyntaxKind::WithCompoundStatement,
                Sequence::new(vec_of_erased![
                    Ref::keyword("WITH"),
                    Ref::keyword("RECURSIVE").optional(),
                    Conditional::new(MetaSegment::indent()).indented_ctes(),
                    Delimited::new(vec_of_erased![Ref::new("CTEDefinitionSegment")]).config(
                        |this| {
                            this.terminators = vec_of_erased![Ref::keyword("SELECT")];
                            this.allow_trailing();
                        }
                    ),
                    Conditional::new(MetaSegment::dedent()).indented_ctes(),
                    one_of(vec_of_erased![
                        Ref::new("NonWithSelectableGrammar"),
                        Ref::new("NonWithNonSelectableGrammar")
                    ])
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "CTEDefinitionSegment".into(),
            NodeMatcher::new(
                SyntaxKind::CommonTableExpression,
                Sequence::new(vec_of_erased![
                    Ref::new("SingleIdentifierGrammar"),
                    Ref::new("CTEColumnList").optional(),
                    Ref::keyword("AS").optional(),
                    Bracketed::new(vec_of_erased![Ref::new("SelectableGrammar")])
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "CTEColumnList".into(),
            NodeMatcher::new(
                SyntaxKind::CTEColumnList,
                Bracketed::new(vec_of_erased![Ref::new("SingleIdentifierListSegment")])
                    .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "SequenceReferenceSegment".into(),
            NodeMatcher::new(
                SyntaxKind::ColumnReference,
                Delimited::new(vec![Ref::new("SingleIdentifierGrammar").boxed()])
                    .config(|this| {
                        this.delimiter(Ref::new("ObjectReferenceDelimiterGrammar"));
                        this.disallow_gaps();
                        this.terminators =
                            vec_of_erased![Ref::new("ObjectReferenceTerminatorGrammar")];
                    })
                    .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "TriggerReferenceSegment".into(),
            NodeMatcher::new(
                SyntaxKind::TriggerReference,
                Delimited::new(vec![Ref::new("SingleIdentifierGrammar").boxed()])
                    .config(|this| {
                        this.delimiter(Ref::new("ObjectReferenceDelimiterGrammar"));
                        this.disallow_gaps();
                        this.terminators =
                            vec_of_erased![Ref::new("ObjectReferenceTerminatorGrammar")];
                    })
                    .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "TableConstraintSegment".into(),
            NodeMatcher::new(
                SyntaxKind::TableConstraint,
                Sequence::new(vec_of_erased![
                    Sequence::new(vec_of_erased![
                        Ref::keyword("CONSTRAINT"),
                        Ref::new("ObjectReferenceSegment")
                    ])
                    .config(|this| this.optional()),
                    one_of(vec_of_erased![
                        Sequence::new(vec_of_erased![
                            Ref::keyword("UNIQUE"),
                            Ref::new("BracketedColumnReferenceListGrammar")
                        ]),
                        Sequence::new(vec_of_erased![
                            Ref::new("PrimaryKeyGrammar"),
                            Ref::new("BracketedColumnReferenceListGrammar")
                        ]),
                        Sequence::new(vec_of_erased![
                            Ref::new("ForeignKeyGrammar"),
                            Ref::new("BracketedColumnReferenceListGrammar"),
                            Ref::new("ReferenceDefinitionGrammar")
                        ])
                    ])
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "JoinOnConditionSegment".into(),
            NodeMatcher::new(
                SyntaxKind::JoinOnCondition,
                Sequence::new(vec_of_erased![
                    Ref::keyword("ON"),
                    Conditional::new(MetaSegment::implicit_indent()).indented_on_contents(),
                    optionally_bracketed(vec_of_erased![Ref::new("ExpressionSegment")]),
                    Conditional::new(MetaSegment::dedent()).indented_on_contents()
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "DatabaseReferenceSegment".into(),
            NodeMatcher::new(
                SyntaxKind::DatabaseReference,
                Delimited::new(vec![Ref::new("SingleIdentifierGrammar").boxed()])
                    .config(|this| {
                        this.delimiter(Ref::new("ObjectReferenceDelimiterGrammar"));
                        this.disallow_gaps();
                        this.terminators =
                            vec_of_erased![Ref::new("ObjectReferenceTerminatorGrammar")];
                    })
                    .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "IndexReferenceSegment".into(),
            NodeMatcher::new(
                SyntaxKind::DatabaseReference,
                Delimited::new(vec![Ref::new("SingleIdentifierGrammar").boxed()])
                    .config(|this| {
                        this.delimiter(Ref::new("ObjectReferenceDelimiterGrammar"));
                        this.disallow_gaps();
                        this.terminators =
                            vec_of_erased![Ref::new("ObjectReferenceTerminatorGrammar")];
                    })
                    .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "CollationReferenceSegment".into(),
            NodeMatcher::new(
                SyntaxKind::CollationReference,
                one_of(vec_of_erased![
                    Ref::new("QuotedLiteralSegment"),
                    Delimited::new(vec_of_erased![Ref::new("SingleIdentifierGrammar")]).config(
                        |this| {
                            this.delimiter(Ref::new("ObjectReferenceDelimiterGrammar"));
                            this.terminators =
                                vec_of_erased![Ref::new("ObjectReferenceTerminatorGrammar")];
                            this.allow_gaps = false;
                        }
                    ),
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "OverClauseSegment".into(),
            NodeMatcher::new(
                SyntaxKind::OverClause,
                Sequence::new(vec_of_erased![
                    MetaSegment::indent(),
                    Ref::new("IgnoreRespectNullsGrammar").optional(),
                    Ref::keyword("OVER"),
                    one_of(vec_of_erased![
                        Ref::new("SingleIdentifierGrammar"),
                        Bracketed::new(vec_of_erased![
                            Ref::new("WindowSpecificationSegment").optional()
                        ])
                        .config(|this| this.parse_mode(ParseMode::Greedy))
                    ]),
                    MetaSegment::dedent()
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "NamedWindowSegment".into(),
            NodeMatcher::new(
                SyntaxKind::NamedWindow,
                Sequence::new(vec_of_erased![
                    Ref::keyword("WINDOW"),
                    MetaSegment::indent(),
                    Delimited::new(vec_of_erased![Ref::new("NamedWindowExpressionSegment")]),
                    MetaSegment::dedent(),
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "WindowSpecificationSegment".into(),
            NodeMatcher::new(
                SyntaxKind::WindowSpecification,
                Sequence::new(vec_of_erased![
                    Ref::new("SingleIdentifierGrammar")
                        .optional()
                        .exclude(Ref::keyword("PARTITION")),
                    Ref::new("PartitionClauseSegment").optional(),
                    Ref::new("OrderByClauseSegment").optional(),
                    Ref::new("FrameClauseSegment").optional()
                ])
                .config(|this| this.optional())
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "PartitionClauseSegment".into(),
            NodeMatcher::new(
                SyntaxKind::PartitionbyClause,
                Sequence::new(vec_of_erased![
                    Ref::keyword("PARTITION"),
                    Ref::keyword("BY"),
                    MetaSegment::indent(),
                    optionally_bracketed(vec_of_erased![Delimited::new(vec_of_erased![Ref::new(
                        "ExpressionSegment"
                    )])]),
                    MetaSegment::dedent()
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "JoinClauseSegment".into(),
            NodeMatcher::new(
                SyntaxKind::JoinClause,
                one_of(vec_of_erased![
                    Sequence::new(vec_of_erased![
                        Ref::new("JoinTypeKeywordsGrammar").optional(),
                        Ref::new("JoinKeywordsGrammar"),
                        MetaSegment::indent(),
                        Ref::new("FromExpressionElementSegment"),
                        AnyNumberOf::new(vec_of_erased![Ref::new("NestedJoinGrammar")]),
                        MetaSegment::dedent(),
                        Sequence::new(vec_of_erased![
                            Conditional::new(MetaSegment::indent()).indented_using_on(),
                            one_of(vec_of_erased![
                                Ref::new("JoinOnConditionSegment"),
                                Sequence::new(vec_of_erased![
                                    Ref::keyword("USING"),
                                    MetaSegment::indent(),
                                    Bracketed::new(vec_of_erased![Delimited::new(vec_of_erased![
                                        Ref::new("SingleIdentifierGrammar")
                                    ])])
                                    .config(|this| this.parse_mode = ParseMode::Greedy),
                                    MetaSegment::dedent(),
                                ])
                            ]),
                            Conditional::new(MetaSegment::dedent()).indented_using_on(),
                        ])
                        .config(|this| this.optional())
                    ]),
                    Sequence::new(vec_of_erased![
                        Ref::new("NaturalJoinKeywordsGrammar"),
                        Ref::new("JoinKeywordsGrammar"),
                        MetaSegment::indent(),
                        Ref::new("FromExpressionElementSegment"),
                        MetaSegment::dedent(),
                    ]),
                    Sequence::new(vec_of_erased![
                        Ref::new("ExtendedNaturalJoinKeywordsGrammar"),
                        MetaSegment::indent(),
                        Ref::new("FromExpressionElementSegment"),
                        MetaSegment::dedent(),
                    ])
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "DropTriggerStatementSegment".into(),
            NodeMatcher::new(
                SyntaxKind::DropTriggerStatement,
                Sequence::new(vec_of_erased![
                    Ref::keyword("DROP"),
                    Ref::keyword("TRIGGER"),
                    Ref::new("IfExistsGrammar").optional(),
                    Ref::new("TriggerReferenceSegment")
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "SamplingExpressionSegment".into(),
            NodeMatcher::new(
                SyntaxKind::SampleExpression,
                Sequence::new(vec_of_erased![
                    Ref::keyword("TABLESAMPLE"),
                    one_of(vec_of_erased![Ref::keyword("BERNOULLI"), Ref::keyword("SYSTEM")]),
                    Bracketed::new(vec_of_erased![Ref::new("NumericLiteralSegment")]),
                    Sequence::new(vec_of_erased![
                        Ref::keyword("REPEATABLE"),
                        Bracketed::new(vec_of_erased![Ref::new("NumericLiteralSegment")]),
                    ])
                    .config(|this| this.optional())
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "TableExpressionSegment".into(),
            NodeMatcher::new(
                SyntaxKind::TableExpression,
                one_of(vec_of_erased![
                    Ref::new("ValuesClauseSegment"),
                    Ref::new("BareFunctionSegment"),
                    Ref::new("FunctionSegment"),
                    Ref::new("TableReferenceSegment"),
                    Bracketed::new(vec_of_erased![Ref::new("SelectableGrammar")]),
                    Bracketed::new(vec_of_erased![Ref::new("MergeStatementSegment")])
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "DropTriggerStatementSegment".into(),
            NodeMatcher::new(
                SyntaxKind::DropTriggerStatement,
                Sequence::new(vec_of_erased![
                    Ref::keyword("DROP"),
                    Ref::keyword("TRIGGER"),
                    Ref::new("IfExistsGrammar").optional(),
                    Ref::new("TriggerReferenceSegment")
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "SamplingExpressionSegment".into(),
            NodeMatcher::new(
                SyntaxKind::SampleExpression,
                Sequence::new(vec_of_erased![
                    Ref::keyword("TABLESAMPLE"),
                    one_of(vec_of_erased![Ref::keyword("BERNOULLI"), Ref::keyword("SYSTEM")]),
                    Bracketed::new(vec_of_erased![Ref::new("NumericLiteralSegment")]),
                    Sequence::new(vec_of_erased![
                        Ref::keyword("REPEATABLE"),
                        Bracketed::new(vec_of_erased![Ref::new("NumericLiteralSegment")]),
                    ])
                    .config(|this| this.optional())
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "TableExpressionSegment".into(),
            NodeMatcher::new(
                SyntaxKind::TableExpression,
                one_of(vec_of_erased![
                    Ref::new("ValuesClauseSegment"),
                    Ref::new("BareFunctionSegment"),
                    Ref::new("FunctionSegment"),
                    Ref::new("TableReferenceSegment"),
                    Bracketed::new(vec_of_erased![Ref::new("SelectableGrammar")]),
                    Bracketed::new(vec_of_erased![Ref::new("MergeStatementSegment")])
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "CreateTriggerStatementSegment".into(),
            NodeMatcher::new(
                SyntaxKind::CreateTriggerStatement,
                Sequence::new(vec![
                    Ref::keyword("CREATE").boxed(),
                    Ref::keyword("TRIGGER").boxed(),
                    Ref::new("TriggerReferenceSegment").boxed(),
                    one_of(vec![
                        Ref::keyword("BEFORE").boxed(),
                        Ref::keyword("AFTER").boxed(),
                        Sequence::new(vec![
                            Ref::keyword("INSTEAD").boxed(),
                            Ref::keyword("OF").boxed(),
                        ])
                        .boxed(),
                    ])
                    .config(|this| this.optional())
                    .boxed(),
                    Delimited::new(vec![
                        Ref::keyword("INSERT").boxed(),
                        Ref::keyword("DELETE").boxed(),
                        Sequence::new(vec![
                            Ref::keyword("UPDATE").boxed(),
                            Ref::keyword("OF").boxed(),
                            Delimited::new(vec![Ref::new("ColumnReferenceSegment").boxed()])
                                //.with_terminators(vec!["OR", "ON"])
                                .boxed(),
                        ])
                        .boxed(),
                    ])
                    .config(|this| {
                        this.delimiter(Ref::keyword("OR"));
                        // .with_terminators(vec!["ON"]);
                    })
                    .boxed(),
                    Ref::keyword("ON").boxed(),
                    Ref::new("TableReferenceSegment").boxed(),
                    AnyNumberOf::new(vec_of_erased![
                        Sequence::new(vec_of_erased![
                            Ref::keyword("REFERENCING"),
                            Ref::keyword("OLD"),
                            Ref::keyword("ROW"),
                            Ref::keyword("AS"),
                            Ref::new("ParameterNameSegment"),
                            Ref::keyword("NEW"),
                            Ref::keyword("ROW"),
                            Ref::keyword("AS"),
                            Ref::new("ParameterNameSegment"),
                        ]),
                        Sequence::new(vec_of_erased![
                            Ref::keyword("FROM"),
                            Ref::new("TableReferenceSegment"),
                        ]),
                        one_of(vec_of_erased![
                            Sequence::new(vec_of_erased![
                                Ref::keyword("NOT"),
                                Ref::keyword("DEFERRABLE"),
                            ]),
                            Sequence::new(vec_of_erased![
                                Ref::keyword("DEFERRABLE").optional(),
                                one_of(vec_of_erased![
                                    Sequence::new(vec_of_erased![
                                        Ref::keyword("INITIALLY"),
                                        Ref::keyword("IMMEDIATE"),
                                    ]),
                                    Sequence::new(vec_of_erased![
                                        Ref::keyword("INITIALLY"),
                                        Ref::keyword("DEFERRED"),
                                    ]),
                                ]),
                            ]),
                        ]),
                        Sequence::new(vec_of_erased![
                            Ref::keyword("FOR"),
                            Ref::keyword("EACH").optional(),
                            one_of(vec_of_erased![Ref::keyword("ROW"), Ref::keyword("STATEMENT"),]),
                        ]),
                        Sequence::new(vec_of_erased![
                            Ref::keyword("WHEN"),
                            Bracketed::new(vec_of_erased![Ref::new("ExpressionSegment"),]),
                        ]),
                    ])
                    .boxed(),
                    Sequence::new(vec![
                        Ref::keyword("EXECUTE").boxed(),
                        Ref::keyword("PROCEDURE").boxed(),
                        Ref::new("FunctionNameIdentifierSegment").boxed(),
                        Bracketed::new(vec![
                            Ref::new("FunctionContentsGrammar").optional().boxed(),
                        ])
                        .boxed(),
                    ])
                    .config(|this| this.optional())
                    .boxed(),
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "DropModelStatementSegment".into(),
            NodeMatcher::new(
                SyntaxKind::DropModelStatement,
                Sequence::new(vec_of_erased![
                    Ref::keyword("DROP"),
                    Ref::keyword("MODEL"),
                    Ref::new("IfExistsGrammar").optional(),
                    Ref::new("ObjectReferenceSegment")
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "DescribeStatementSegment".into(),
            NodeMatcher::new(
                SyntaxKind::DescribeStatement,
                Sequence::new(vec_of_erased![
                    Ref::keyword("DESCRIBE"),
                    Ref::new("NakedIdentifierSegment"),
                    Ref::new("ObjectReferenceSegment")
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "UseStatementSegment".into(),
            NodeMatcher::new(
                SyntaxKind::UseStatement,
                Sequence::new(vec_of_erased![
                    Ref::keyword("USE"),
                    Ref::new("DatabaseReferenceSegment")
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "ExplainStatementSegment".into(),
            NodeMatcher::new(
                SyntaxKind::ExplainStatement,
                Sequence::new(vec_of_erased![
                    Ref::keyword("EXPLAIN"),
                    one_of(vec_of_erased![
                        Ref::new("SelectableGrammar"),
                        Ref::new("InsertStatementSegment"),
                        Ref::new("UpdateStatementSegment"),
                        Ref::new("DeleteStatementSegment")
                    ])
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "CreateSequenceStatementSegment".into(),
            NodeMatcher::new(
                SyntaxKind::CreateSequenceStatement,
                Sequence::new(vec_of_erased![
                    Ref::keyword("CREATE"),
                    Ref::keyword("SEQUENCE"),
                    Ref::new("SequenceReferenceSegment"),
                    AnyNumberOf::new(vec_of_erased![Ref::new("CreateSequenceOptionsSegment")])
                        .config(|this| this.optional())
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "CreateSequenceOptionsSegment".into(),
            NodeMatcher::new(
                SyntaxKind::CreateSequenceOptionsSegment,
                one_of(vec_of_erased![
                    Sequence::new(vec_of_erased![
                        Ref::keyword("INCREMENT"),
                        Ref::keyword("BY"),
                        Ref::new("NumericLiteralSegment")
                    ]),
                    Sequence::new(vec_of_erased![
                        Ref::keyword("START"),
                        Ref::keyword("WITH").optional(),
                        Ref::new("NumericLiteralSegment")
                    ]),
                    one_of(vec_of_erased![
                        Sequence::new(vec_of_erased![
                            Ref::keyword("MINVALUE"),
                            Ref::new("NumericLiteralSegment")
                        ]),
                        Sequence::new(vec_of_erased![Ref::keyword("NO"), Ref::keyword("MINVALUE")])
                    ]),
                    one_of(vec_of_erased![
                        Sequence::new(vec_of_erased![
                            Ref::keyword("MAXVALUE"),
                            Ref::new("NumericLiteralSegment")
                        ]),
                        Sequence::new(vec_of_erased![Ref::keyword("NO"), Ref::keyword("MAXVALUE")])
                    ]),
                    one_of(vec_of_erased![
                        Sequence::new(vec_of_erased![
                            Ref::keyword("CACHE"),
                            Ref::new("NumericLiteralSegment")
                        ]),
                        Ref::keyword("NOCACHE")
                    ]),
                    one_of(vec_of_erased![Ref::keyword("CYCLE"), Ref::keyword("NOCYCLE")]),
                    one_of(vec_of_erased![Ref::keyword("ORDER"), Ref::keyword("NOORDER")])
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "AlterSequenceStatementSegment".into(),
            NodeMatcher::new(
                SyntaxKind::AlterSequenceStatement,
                Sequence::new(vec_of_erased![
                    Ref::keyword("ALTER"),
                    Ref::keyword("SEQUENCE"),
                    Ref::new("SequenceReferenceSegment"),
                    AnyNumberOf::new(vec_of_erased![Ref::new("AlterSequenceOptionsSegment")])
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "DropSequenceStatementSegment".into(),
            NodeMatcher::new(
                SyntaxKind::DropSequenceStatement,
                Sequence::new(vec_of_erased![
                    Ref::keyword("DROP"),
                    Ref::keyword("SEQUENCE"),
                    Ref::new("SequenceReferenceSegment")
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "DropCastStatementSegment".into(),
            NodeMatcher::new(
                SyntaxKind::DropCastStatement,
                Sequence::new(vec_of_erased![
                    Ref::keyword("DROP"),
                    Ref::keyword("CAST"),
                    Bracketed::new(vec_of_erased![
                        Ref::new("DatatypeSegment"),
                        Ref::keyword("AS"),
                        Ref::new("DatatypeSegment")
                    ]),
                    Ref::new("DropBehaviorGrammar").optional()
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "CreateFunctionStatementSegment".into(),
            NodeMatcher::new(
                SyntaxKind::CreateFunctionStatement,
                Sequence::new(vec_of_erased![
                    Ref::keyword("CREATE"),
                    Ref::new("OrReplaceGrammar").optional(),
                    Ref::new("TemporaryGrammar").optional(),
                    Ref::keyword("FUNCTION"),
                    Ref::new("IfNotExistsGrammar").optional(),
                    Ref::new("FunctionNameSegment"),
                    Ref::new("FunctionParameterListGrammar"),
                    Sequence::new(vec_of_erased![
                        Ref::keyword("RETURNS"),
                        Ref::new("DatatypeSegment")
                    ])
                    .config(|this| this.optional()),
                    Ref::new("FunctionDefinitionGrammar")
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "DropFunctionStatementSegment".into(),
            NodeMatcher::new(
                SyntaxKind::DropFunctionStatement,
                Sequence::new(vec_of_erased![
                    Ref::keyword("DROP"),
                    Ref::keyword("FUNCTION"),
                    Ref::new("IfExistsGrammar").optional(),
                    Ref::new("FunctionNameSegment")
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "CreateModelStatementSegment".into(),
            NodeMatcher::new(
                SyntaxKind::CreateModelStatement,
                Sequence::new(vec_of_erased![
                    Ref::keyword("CREATE"),
                    Ref::new("OrReplaceGrammar").optional(),
                    Ref::keyword("MODEL"),
                    Ref::new("IfNotExistsGrammar").optional(),
                    Ref::new("ObjectReferenceSegment"),
                    Sequence::new(vec_of_erased![
                        Ref::keyword("OPTIONS"),
                        Bracketed::new(vec_of_erased![Delimited::new(vec_of_erased![
                            Sequence::new(vec_of_erased![
                                Ref::new("ParameterNameSegment"),
                                Ref::new("EqualsSegment"),
                                one_of(vec_of_erased![
                                    Ref::new("LiteralGrammar"), // Single value
                                    Bracketed::new(vec_of_erased![Delimited::new(vec_of_erased![
                                        Ref::new("QuotedLiteralSegment")
                                    ])])
                                    .config(|this| {
                                        this.bracket_type("square");
                                        this.optional();
                                    })
                                ])
                            ])
                        ])])
                    ])
                    .config(|this| this.optional()),
                    Ref::keyword("AS"),
                    Ref::new("SelectableGrammar")
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "CreateViewStatementSegment".into(),
            NodeMatcher::new(
                SyntaxKind::CreateViewStatement,
                Sequence::new(vec_of_erased![
                    Ref::keyword("CREATE"),
                    Ref::new("OrReplaceGrammar").optional(),
                    Ref::keyword("VIEW"),
                    Ref::new("IfNotExistsGrammar").optional(),
                    Ref::new("TableReferenceSegment"),
                    Ref::new("BracketedColumnReferenceListGrammar").optional(),
                    Ref::keyword("AS"),
                    optionally_bracketed(vec_of_erased![Ref::new("SelectableGrammar")]),
                    Ref::new("WithNoSchemaBindingClauseSegment").optional()
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "DeleteStatementSegment".into(),
            NodeMatcher::new(
                SyntaxKind::DeleteStatement,
                Sequence::new(vec_of_erased![
                    Ref::keyword("DELETE"),
                    Ref::new("FromClauseSegment"),
                    Ref::new("WhereClauseSegment").optional()
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "UpdateStatementSegment".into(),
            NodeMatcher::new(
                SyntaxKind::UpdateStatement,
                Sequence::new(vec_of_erased![
                    Ref::keyword("UPDATE"),
                    Ref::new("TableReferenceSegment"),
                    Ref::new("AliasExpressionSegment").exclude(Ref::keyword("SET")).optional(),
                    Ref::new("SetClauseListSegment"),
                    Ref::new("FromClauseSegment").optional(),
                    Ref::new("WhereClauseSegment").optional()
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "CreateCastStatementSegment".into(),
            NodeMatcher::new(
                SyntaxKind::CreateCastStatement,
                Sequence::new(vec_of_erased![
                    Ref::keyword("CREATE"),
                    Ref::keyword("CAST"),
                    Bracketed::new(vec_of_erased![
                        Ref::new("DatatypeSegment"),
                        Ref::keyword("AS"),
                        Ref::new("DatatypeSegment")
                    ]),
                    Ref::keyword("WITH"),
                    Ref::keyword("SPECIFIC").optional(),
                    one_of(vec_of_erased![
                        Ref::keyword("ROUTINE"),
                        Ref::keyword("FUNCTION"),
                        Ref::keyword("PROCEDURE"),
                        Sequence::new(vec_of_erased![
                            one_of(vec_of_erased![
                                Ref::keyword("INSTANCE"),
                                Ref::keyword("STATIC"),
                                Ref::keyword("CONSTRUCTOR")
                            ])
                            .config(|this| this.optional()),
                            Ref::keyword("METHOD")
                        ])
                    ]),
                    Ref::new("FunctionNameSegment"),
                    Ref::new("FunctionParameterListGrammar").optional(),
                    Sequence::new(vec_of_erased![
                        Ref::keyword("FOR"),
                        Ref::new("ObjectReferenceSegment")
                    ])
                    .config(|this| this.optional()),
                    Sequence::new(vec_of_erased![Ref::keyword("AS"), Ref::keyword("ASSIGNMENT")])
                        .config(|this| this.optional())
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "CreateRoleStatementSegment".into(),
            NodeMatcher::new(
                SyntaxKind::CreateRoleStatement,
                Sequence::new(vec_of_erased![
                    Ref::keyword("CREATE"),
                    Ref::keyword("ROLE"),
                    Ref::new("RoleReferenceSegment")
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "DropRoleStatementSegment".into(),
            NodeMatcher::new(
                SyntaxKind::DropRoleStatement,
                Sequence::new(vec_of_erased![
                    Ref::keyword("DROP"),
                    Ref::keyword("ROLE"),
                    Ref::new("IfExistsGrammar").optional(),
                    Ref::new("SingleIdentifierGrammar")
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "AlterTableStatementSegment".into(),
            NodeMatcher::new(
                SyntaxKind::AlterTableStatement,
                Sequence::new(vec_of_erased![
                    Ref::keyword("ALTER"),
                    Ref::keyword("TABLE"),
                    Ref::new("TableReferenceSegment"),
                    Delimited::new(vec_of_erased![Ref::new("AlterTableOptionsGrammar")])
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "SetSchemaStatementSegment".into(),
            NodeMatcher::new(
                SyntaxKind::SetSchemaStatement,
                Sequence::new(vec_of_erased![
                    Ref::keyword("SET"),
                    Ref::keyword("SCHEMA"),
                    Ref::new("IfNotExistsGrammar").optional(),
                    Ref::new("SchemaReferenceSegment")
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "DropSchemaStatementSegment".into(),
            NodeMatcher::new(
                SyntaxKind::DropSchemaStatement,
                Sequence::new(vec_of_erased![
                    Ref::keyword("DROP"),
                    Ref::keyword("SCHEMA"),
                    Ref::new("IfExistsGrammar").optional(),
                    Ref::new("SchemaReferenceSegment"),
                    Ref::new("DropBehaviorGrammar").optional()
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "DropTypeStatementSegment".into(),
            NodeMatcher::new(
                SyntaxKind::DropTypeStatement,
                Sequence::new(vec_of_erased![
                    Ref::keyword("DROP"),
                    Ref::keyword("TYPE"),
                    Ref::new("IfExistsGrammar").optional(),
                    Ref::new("ObjectReferenceSegment"),
                    Ref::new("DropBehaviorGrammar").optional()
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "CreateDatabaseStatementSegment".into(),
            NodeMatcher::new(
                SyntaxKind::CreateDatabaseStatement,
                Sequence::new(vec_of_erased![
                    Ref::keyword("CREATE"),
                    Ref::keyword("DATABASE"),
                    Ref::new("IfNotExistsGrammar").optional(),
                    Ref::new("DatabaseReferenceSegment")
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "DropDatabaseStatementSegment".into(),
            NodeMatcher::new(
                SyntaxKind::DropDatabaseStatement,
                Sequence::new(vec_of_erased![
                    Ref::keyword("DROP"),
                    Ref::keyword("DATABASE"),
                    Ref::new("IfExistsGrammar").optional(),
                    Ref::new("DatabaseReferenceSegment"),
                    Ref::new("DropBehaviorGrammar").optional()
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "FunctionParameterListGrammar".into(),
            NodeMatcher::new(
                SyntaxKind::FunctionParameterList,
                Bracketed::new(vec_of_erased![
                    Delimited::new(vec_of_erased![Ref::new("FunctionParameterGrammar")])
                        .config(|this| this.optional())
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "CreateIndexStatementSegment".into(),
            NodeMatcher::new(
                SyntaxKind::CreateIndexStatement,
                Sequence::new(vec_of_erased![
                    Ref::keyword("CREATE"),
                    Ref::new("OrReplaceGrammar").optional(),
                    Ref::keyword("UNIQUE").optional(),
                    Ref::keyword("INDEX"),
                    Ref::new("IfNotExistsGrammar").optional(),
                    Ref::new("IndexReferenceSegment"),
                    Ref::keyword("ON"),
                    Ref::new("TableReferenceSegment"),
                    Bracketed::new(vec_of_erased![Delimited::new(vec_of_erased![Ref::new(
                        "IndexColumnDefinitionSegment"
                    )])])
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "DropIndexStatementSegment".into(),
            NodeMatcher::new(
                SyntaxKind::DropIndexStatement,
                Sequence::new(vec_of_erased![
                    Ref::keyword("DROP"),
                    Ref::keyword("INDEX"),
                    Ref::new("IfExistsGrammar").optional(),
                    Ref::new("IndexReferenceSegment"),
                    Ref::new("DropBehaviorGrammar").optional()
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "CreateTableStatementSegment".into(),
            NodeMatcher::new(
                SyntaxKind::CreateTableStatement,
                Sequence::new(vec_of_erased![
                    Ref::keyword("CREATE"),
                    Ref::new("OrReplaceGrammar").optional(),
                    Ref::new("TemporaryTransientGrammar").optional(),
                    Ref::keyword("TABLE"),
                    Ref::new("IfNotExistsGrammar").optional(),
                    Ref::new("TableReferenceSegment"),
                    one_of(vec_of_erased![
                        // Columns and comment syntax
                        Sequence::new(vec_of_erased![
                            Bracketed::new(vec_of_erased![Delimited::new(vec_of_erased![one_of(
                                vec_of_erased![
                                    Ref::new("TableConstraintSegment"),
                                    Ref::new("ColumnDefinitionSegment")
                                ]
                            )])]),
                            Ref::new("CommentClauseSegment").optional()
                        ]),
                        // Create AS syntax:
                        Sequence::new(vec_of_erased![
                            Ref::keyword("AS"),
                            optionally_bracketed(vec_of_erased![Ref::new("SelectableGrammar")])
                        ]),
                        // Create LIKE syntax
                        Sequence::new(vec_of_erased![
                            Ref::keyword("LIKE"),
                            Ref::new("TableReferenceSegment")
                        ])
                    ]),
                    Ref::new("TableEndClauseSegment").optional()
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "AccessStatementSegment".into(),
            NodeMatcher::new(
                SyntaxKind::AccessStatement,
                {
                    let global_permissions = one_of(vec_of_erased![
                        Sequence::new(vec_of_erased![
                            Ref::keyword("CREATE"),
                            one_of(vec_of_erased![
                                Ref::keyword("ROLE"),
                                Ref::keyword("USER"),
                                Ref::keyword("WAREHOUSE"),
                                Ref::keyword("DATABASE"),
                                Ref::keyword("INTEGRATION"),
                            ]),
                        ]),
                        Sequence::new(vec_of_erased![
                            Ref::keyword("APPLY"),
                            Ref::keyword("MASKING"),
                            Ref::keyword("POLICY"),
                        ]),
                        Sequence::new(vec_of_erased![
                            Ref::keyword("EXECUTE"),
                            Ref::keyword("TASK")
                        ]),
                        Sequence::new(vec_of_erased![
                            Ref::keyword("MANAGE"),
                            Ref::keyword("GRANTS")
                        ]),
                        Sequence::new(vec_of_erased![
                            Ref::keyword("MONITOR"),
                            one_of(vec_of_erased![
                                Ref::keyword("EXECUTION"),
                                Ref::keyword("USAGE")
                            ]),
                        ]),
                    ]);

                    let schema_object_types = one_of(vec_of_erased![
                        Ref::keyword("TABLE"),
                        Ref::keyword("VIEW"),
                        Ref::keyword("STAGE"),
                        Ref::keyword("FUNCTION"),
                        Ref::keyword("PROCEDURE"),
                        Ref::keyword("ROUTINE"),
                        Ref::keyword("SEQUENCE"),
                        Ref::keyword("STREAM"),
                        Ref::keyword("TASK"),
                    ]);

                    let permissions = Sequence::new(vec_of_erased![
                        one_of(vec_of_erased![
                            Sequence::new(vec_of_erased![
                                Ref::keyword("CREATE"),
                                one_of(vec_of_erased![
                                    Ref::keyword("SCHEMA"),
                                    Sequence::new(vec_of_erased![
                                        Ref::keyword("MASKING"),
                                        Ref::keyword("POLICY"),
                                    ]),
                                    Ref::keyword("PIPE"),
                                    schema_object_types.clone(),
                                ]),
                            ]),
                            Sequence::new(vec_of_erased![
                                Ref::keyword("IMPORTED"),
                                Ref::keyword("PRIVILEGES")
                            ]),
                            Ref::keyword("APPLY"),
                            Ref::keyword("CONNECT"),
                            Ref::keyword("CREATE"),
                            Ref::keyword("DELETE"),
                            Ref::keyword("EXECUTE"),
                            Ref::keyword("INSERT"),
                            Ref::keyword("MODIFY"),
                            Ref::keyword("MONITOR"),
                            Ref::keyword("OPERATE"),
                            Ref::keyword("OWNERSHIP"),
                            Ref::keyword("READ"),
                            Ref::keyword("REFERENCE_USAGE"),
                            Ref::keyword("REFERENCES"),
                            Ref::keyword("SELECT"),
                            Ref::keyword("TEMP"),
                            Ref::keyword("TEMPORARY"),
                            Ref::keyword("TRIGGER"),
                            Ref::keyword("TRUNCATE"),
                            Ref::keyword("UPDATE"),
                            Ref::keyword("USAGE"),
                            Ref::keyword("USE_ANY_ROLE"),
                            Ref::keyword("WRITE"),
                            Sequence::new(vec_of_erased![
                                Ref::keyword("ALL"),
                                Ref::keyword("PRIVILEGES").optional(),
                            ]),
                        ]),
                        Ref::new("BracketedColumnReferenceListGrammar").optional(),
                    ]);

                    let objects = one_of(vec_of_erased![
                        Ref::keyword("ACCOUNT"),
                        Sequence::new(vec_of_erased![
                            one_of(vec_of_erased![
                                Sequence::new(vec_of_erased![
                                    Ref::keyword("RESOURCE"),
                                    Ref::keyword("MONITOR"),
                                ]),
                                Ref::keyword("WAREHOUSE"),
                                Ref::keyword("DATABASE"),
                                Ref::keyword("DOMAIN"),
                                Ref::keyword("INTEGRATION"),
                                Ref::keyword("LANGUAGE"),
                                Ref::keyword("SCHEMA"),
                                Ref::keyword("ROLE"),
                                Ref::keyword("TABLESPACE"),
                                Ref::keyword("TYPE"),
                                Sequence::new(vec_of_erased![
                                    Ref::keyword("FOREIGN"),
                                    one_of(vec_of_erased![
                                        Ref::keyword("SERVER"),
                                        Sequence::new(vec_of_erased![
                                            Ref::keyword("DATA"),
                                            Ref::keyword("WRAPPER"),
                                        ]),
                                    ]),
                                ]),
                                Sequence::new(vec_of_erased![
                                    Ref::keyword("ALL"),
                                    Ref::keyword("SCHEMAS"),
                                    Ref::keyword("IN"),
                                    Ref::keyword("DATABASE"),
                                ]),
                                Sequence::new(vec_of_erased![
                                    Ref::keyword("FUTURE"),
                                    Ref::keyword("SCHEMAS"),
                                    Ref::keyword("IN"),
                                    Ref::keyword("DATABASE"),
                                ]),
                                schema_object_types.clone(),
                                Sequence::new(vec_of_erased![
                                    Ref::keyword("ALL"),
                                    one_of(vec_of_erased![
                                        Ref::keyword("TABLES"),
                                        Ref::keyword("VIEWS"),
                                        Ref::keyword("STAGES"),
                                        Ref::keyword("FUNCTIONS"),
                                        Ref::keyword("PROCEDURES"),
                                        Ref::keyword("ROUTINES"),
                                        Ref::keyword("SEQUENCES"),
                                        Ref::keyword("STREAMS"),
                                        Ref::keyword("TASKS"),
                                    ]),
                                    Ref::keyword("IN"),
                                    Ref::keyword("SCHEMA"),
                                ]),
                                Sequence::new(vec_of_erased![
                                    Ref::keyword("FUTURE"),
                                    Ref::keyword("IN"),
                                    one_of(vec_of_erased![
                                        Ref::keyword("DATABASE"),
                                        Ref::keyword("SCHEMA")
                                    ]),
                                ]),
                            ])
                            .config(|this| this.optional()),
                            Delimited::new(vec_of_erased![
                                Ref::new("ObjectReferenceSegment"),
                                Sequence::new(vec_of_erased![
                                    Ref::new("FunctionNameSegment"),
                                    Ref::new("FunctionParameterListGrammar").optional(),
                                ]),
                            ])
                            .config(|this| this.terminators =
                                vec_of_erased![Ref::keyword("TO"), Ref::keyword("FROM")]),
                        ]),
                        Sequence::new(vec_of_erased![
                            Ref::keyword("LARGE"),
                            Ref::keyword("OBJECT"),
                            Ref::new("NumericLiteralSegment"),
                        ]),
                    ]);

                    one_of(vec_of_erased![
                        Sequence::new(vec_of_erased![
                            Ref::keyword("GRANT"),
                            one_of(vec_of_erased![
                                Sequence::new(vec_of_erased![
                                    Delimited::new(vec_of_erased![one_of(vec_of_erased![
                                        global_permissions.clone(),
                                        permissions.clone()
                                    ])])
                                    .config(|this| this.terminators =
                                        vec_of_erased![Ref::keyword("ON")]),
                                    Ref::keyword("ON"),
                                    objects.clone()
                                ]),
                                Sequence::new(vec_of_erased![
                                    Ref::keyword("ROLE"),
                                    Ref::new("ObjectReferenceSegment")
                                ]),
                                Sequence::new(vec_of_erased![
                                    Ref::keyword("OWNERSHIP"),
                                    Ref::keyword("ON"),
                                    Ref::keyword("USER"),
                                    Ref::new("ObjectReferenceSegment"),
                                ]),
                                Ref::new("ObjectReferenceSegment")
                            ]),
                            Ref::keyword("TO"),
                            one_of(vec_of_erased![
                                Ref::keyword("GROUP"),
                                Ref::keyword("USER"),
                                Ref::keyword("ROLE"),
                                Ref::keyword("SHARE")
                            ])
                            .config(|this| this.optional()),
                            Delimited::new(vec_of_erased![one_of(vec_of_erased![
                                Ref::new("RoleReferenceSegment"),
                                Ref::new("FunctionSegment"),
                                Ref::keyword("PUBLIC")
                            ])]),
                            one_of(vec_of_erased![
                                Sequence::new(vec_of_erased![
                                    Ref::keyword("WITH"),
                                    Ref::keyword("GRANT"),
                                    Ref::keyword("OPTION"),
                                ]),
                                Sequence::new(vec_of_erased![
                                    Ref::keyword("WITH"),
                                    Ref::keyword("ADMIN"),
                                    Ref::keyword("OPTION"),
                                ]),
                                Sequence::new(vec_of_erased![
                                    Ref::keyword("COPY"),
                                    Ref::keyword("CURRENT"),
                                    Ref::keyword("GRANTS"),
                                ])
                            ])
                            .config(|this| this.optional()),
                            Sequence::new(vec_of_erased![
                                Ref::keyword("GRANTED"),
                                Ref::keyword("BY"),
                                one_of(vec_of_erased![
                                    Ref::keyword("CURRENT_USER"),
                                    Ref::keyword("SESSION_USER"),
                                    Ref::new("ObjectReferenceSegment")
                                ])
                            ])
                            .config(|this| this.optional())
                        ]),
                        Sequence::new(vec_of_erased![
                            Ref::keyword("REVOKE"),
                            Sequence::new(vec_of_erased![
                                Ref::keyword("GRANT"),
                                Ref::keyword("OPTION"),
                                Ref::keyword("FOR")
                            ])
                            .config(|this| this.optional()),
                            one_of(vec_of_erased![
                                Sequence::new(vec_of_erased![
                                    Delimited::new(vec_of_erased![
                                        one_of(vec_of_erased![global_permissions, permissions])
                                            .config(|this| this.terminators =
                                                vec_of_erased![Ref::keyword("ON")])
                                    ]),
                                    Ref::keyword("ON"),
                                    objects
                                ]),
                                Sequence::new(vec_of_erased![
                                    Ref::keyword("ROLE"),
                                    Ref::new("ObjectReferenceSegment")
                                ]),
                                Sequence::new(vec_of_erased![
                                    Ref::keyword("OWNERSHIP"),
                                    Ref::keyword("ON"),
                                    Ref::keyword("USER"),
                                    Ref::new("ObjectReferenceSegment"),
                                ]),
                                Ref::new("ObjectReferenceSegment"),
                            ]),
                            Ref::keyword("FROM"),
                            one_of(vec_of_erased![
                                Ref::keyword("GROUP"),
                                Ref::keyword("USER"),
                                Ref::keyword("ROLE"),
                                Ref::keyword("SHARE")
                            ])
                            .config(|this| this.optional()),
                            Delimited::new(vec_of_erased![Ref::new("ObjectReferenceSegment")]),
                            Ref::new("DropBehaviorGrammar").optional()
                        ])
                    ])
                }
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "InsertStatementSegment".into(),
            NodeMatcher::new(
                SyntaxKind::InsertStatement,
                Sequence::new(vec_of_erased![
                    Ref::keyword("INSERT"),
                    Ref::keyword("OVERWRITE").optional(),
                    Ref::keyword("INTO"),
                    Ref::new("TableReferenceSegment"),
                    one_of(vec_of_erased![
                        Ref::new("SelectableGrammar"),
                        Sequence::new(vec_of_erased![
                            Ref::new("BracketedColumnReferenceListGrammar"),
                            Ref::new("SelectableGrammar")
                        ]),
                        Ref::new("DefaultValuesGrammar")
                    ])
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "TransactionStatementSegment".into(),
            NodeMatcher::new(
                SyntaxKind::TransactionStatement,
                Sequence::new(vec_of_erased![
                    one_of(vec_of_erased![
                        Ref::keyword("START"),
                        Ref::keyword("BEGIN"),
                        Ref::keyword("COMMIT"),
                        Ref::keyword("ROLLBACK"),
                        Ref::keyword("END")
                    ]),
                    one_of(vec_of_erased![Ref::keyword("TRANSACTION"), Ref::keyword("WORK")])
                        .config(|this| this.optional()),
                    Sequence::new(vec_of_erased![
                        Ref::keyword("NAME"),
                        Ref::new("SingleIdentifierGrammar")
                    ])
                    .config(|this| this.optional()),
                    Sequence::new(vec_of_erased![
                        Ref::keyword("AND"),
                        Ref::keyword("NO").optional(),
                        Ref::keyword("CHAIN")
                    ])
                    .config(|this| this.optional())
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "DropTableStatementSegment".into(),
            NodeMatcher::new(
                SyntaxKind::DropTableStatement,
                Sequence::new(vec_of_erased![
                    Ref::keyword("DROP"),
                    Ref::new("TemporaryGrammar").optional(),
                    Ref::keyword("TABLE"),
                    Ref::new("IfExistsGrammar").optional(),
                    Delimited::new(vec_of_erased![Ref::new("TableReferenceSegment")]),
                    Ref::new("DropBehaviorGrammar").optional()
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "DropViewStatementSegment".into(),
            NodeMatcher::new(
                SyntaxKind::DropViewStatement,
                Sequence::new(vec_of_erased![
                    Ref::keyword("DROP"),
                    Ref::keyword("VIEW"),
                    Ref::new("IfExistsGrammar").optional(),
                    Ref::new("TableReferenceSegment"),
                    Ref::new("DropBehaviorGrammar").optional()
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "CreateUserStatementSegment".into(),
            NodeMatcher::new(
                SyntaxKind::CreateUserStatement,
                Sequence::new(vec_of_erased![
                    Ref::keyword("CREATE"),
                    Ref::keyword("USER"),
                    Ref::new("RoleReferenceSegment")
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "DropUserStatementSegment".into(),
            NodeMatcher::new(
                SyntaxKind::DropUserStatement,
                Sequence::new(vec_of_erased![
                    Ref::keyword("DROP"),
                    Ref::keyword("USER"),
                    Ref::new("IfExistsGrammar").optional(),
                    Ref::new("RoleReferenceSegment")
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "NotEqualToSegment".into(),
            NodeMatcher::new(
                SyntaxKind::ComparisonOperator,
                one_of(vec![
                    Sequence::new(vec![
                        Ref::new("RawNotSegment").boxed(),
                        Ref::new("RawEqualsSegment").boxed(),
                    ])
                    .allow_gaps(false)
                    .boxed(),
                    Sequence::new(vec![
                        Ref::new("RawLessThanSegment").boxed(),
                        Ref::new("RawGreaterThanSegment").boxed(),
                    ])
                    .allow_gaps(false)
                    .boxed(),
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "ConcatSegment".into(),
            NodeMatcher::new(
                SyntaxKind::BinaryOperator,
                Sequence::new(vec![
                    Ref::new("PipeSegment").boxed(),
                    Ref::new("PipeSegment").boxed(),
                ])
                .allow_gaps(false)
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "ArrayExpressionSegment".into(),
            NodeMatcher::new(SyntaxKind::ArrayExpression, Nothing::new().to_matchable())
                .to_matchable()
                .into(),
        ),
        (
            "LocalAliasSegment".into(),
            NodeMatcher::new(SyntaxKind::LocalAlias, Nothing::new().to_matchable())
                .to_matchable()
                .into(),
        ),
        (
            "MergeStatementSegment".into(),
            NodeMatcher::new(
                SyntaxKind::MergeStatement,
                Sequence::new(vec![
                    Ref::new("MergeIntoLiteralGrammar").boxed(),
                    MetaSegment::indent().boxed(),
                    one_of(vec![
                        Ref::new("TableReferenceSegment").boxed(),
                        Ref::new("AliasedTableReferenceGrammar").boxed(),
                    ])
                    .boxed(),
                    MetaSegment::dedent().boxed(),
                    Ref::keyword("USING").boxed(),
                    MetaSegment::indent().boxed(),
                    one_of(vec![
                        Ref::new("TableReferenceSegment").boxed(),
                        Ref::new("AliasedTableReferenceGrammar").boxed(),
                        Sequence::new(vec![
                            Bracketed::new(vec![Ref::new("SelectableGrammar").boxed()]).boxed(),
                            Ref::new("AliasExpressionSegment").optional().boxed(),
                        ])
                        .boxed(),
                    ])
                    .boxed(),
                    MetaSegment::dedent().boxed(),
                    Conditional::new(MetaSegment::indent()).indented_using_on().boxed(),
                    Ref::new("JoinOnConditionSegment").boxed(),
                    Conditional::new(MetaSegment::dedent()).indented_using_on().boxed(),
                    Ref::new("MergeMatchSegment").boxed(),
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "IndexColumnDefinitionSegment".into(),
            NodeMatcher::new(
                SyntaxKind::IndexColumnDefinition,
                Sequence::new(vec![
                    Ref::new("SingleIdentifierGrammar").boxed(), // Column name
                    one_of(vec![Ref::keyword("ASC").boxed(), Ref::keyword("DESC").boxed()])
                        .config(|this| this.optional())
                        .boxed(),
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "BitwiseAndSegment".into(),
            NodeMatcher::new(
                SyntaxKind::ComparisonOperator,
                Ref::new("AmpersandSegment").to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "BitwiseOrSegment".into(),
            NodeMatcher::new(
                SyntaxKind::ComparisonOperator,
                Ref::new("PipeSegment").to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "BitwiseLShiftSegment".into(),
            NodeMatcher::new(
                SyntaxKind::ComparisonOperator,
                Sequence::new(vec![
                    Ref::new("RawLessThanSegment").boxed(),
                    Ref::new("RawLessThanSegment").boxed(),
                ])
                .allow_gaps(false)
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "BitwiseRShiftSegment".into(),
            NodeMatcher::new(
                SyntaxKind::ComparisonOperator,
                Sequence::new(vec![
                    Ref::new("RawGreaterThanSegment").boxed(),
                    Ref::new("RawGreaterThanSegment").boxed(),
                ])
                .allow_gaps(false)
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "LessThanSegment".into(),
            NodeMatcher::new(
                SyntaxKind::ComparisonOperator,
                Ref::new("RawLessThanSegment").to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "GreaterThanOrEqualToSegment".into(),
            NodeMatcher::new(
                SyntaxKind::ComparisonOperator,
                Sequence::new(vec![
                    Ref::new("RawGreaterThanSegment").boxed(),
                    Ref::new("RawEqualsSegment").boxed(),
                ])
                .allow_gaps(false)
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "LessThanOrEqualToSegment".into(),
            NodeMatcher::new(
                SyntaxKind::ComparisonOperator,
                Sequence::new(vec![
                    Ref::new("RawLessThanSegment").boxed(),
                    Ref::new("RawEqualsSegment").boxed(),
                ])
                .allow_gaps(false)
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "EqualsSegment".into(),
            NodeMatcher::new(
                SyntaxKind::ComparisonOperator,
                Ref::new("RawEqualsSegment").to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "GreaterThanSegment".into(),
            NodeMatcher::new(
                SyntaxKind::ComparisonOperator,
                Ref::new("RawGreaterThanSegment").to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "QualifiedNumericLiteralSegment".into(),
            NodeMatcher::new(
                SyntaxKind::NumericLiteral,
                Sequence::new(vec![
                    Ref::new("SignedSegmentGrammar").boxed(),
                    Ref::new("NumericLiteralSegment").boxed(),
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "AggregateOrderByClause".into(),
            NodeMatcher::new(
                SyntaxKind::AggregateOrderByClause,
                Ref::new("OrderByClauseSegment").to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "FunctionNameSegment".into(),
            NodeMatcher::new(
                SyntaxKind::FunctionName,
                Sequence::new(vec_of_erased![
                    AnyNumberOf::new(vec_of_erased![Sequence::new(vec_of_erased![
                        Ref::new("SingleIdentifierGrammar"),
                        Ref::new("DotSegment")
                    ])])
                    .config(|this| this.terminators = vec_of_erased![Ref::new("BracketedSegment")]),
                    one_of(vec_of_erased![
                        Ref::new("FunctionNameIdentifierSegment"),
                        Ref::new("QuotedIdentifierSegment")
                    ])
                ])
                .terminators(vec_of_erased![Ref::new("BracketedSegment")])
                .allow_gaps(false)
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "CaseExpressionSegment".into(),
            NodeMatcher::new(
                SyntaxKind::CaseExpression,
                one_of(vec_of_erased![
                    Sequence::new(vec_of_erased![
                        Ref::keyword("CASE"),
                        MetaSegment::implicit_indent(),
                        AnyNumberOf::new(vec_of_erased![Ref::new("WhenClauseSegment")],).config(
                            |this| {
                                this.reset_terminators = true;
                                this.terminators =
                                    vec_of_erased![Ref::keyword("ELSE"), Ref::keyword("END")];
                            }
                        ),
                        Ref::new("ElseClauseSegment").optional(),
                        MetaSegment::dedent(),
                        Ref::keyword("END"),
                    ]),
                    Sequence::new(vec_of_erased![
                        Ref::keyword("CASE"),
                        Ref::new("ExpressionSegment"),
                        MetaSegment::implicit_indent(),
                        AnyNumberOf::new(vec_of_erased![Ref::new("WhenClauseSegment")],).config(
                            |this| {
                                this.reset_terminators = true;
                                this.terminators =
                                    vec_of_erased![Ref::keyword("ELSE"), Ref::keyword("END")];
                            }
                        ),
                        Ref::new("ElseClauseSegment").optional(),
                        MetaSegment::dedent(),
                        Ref::keyword("END"),
                    ]),
                ])
                .config(|this| {
                    this.terminators = vec_of_erased![
                        Ref::new("ComparisonOperatorGrammar"),
                        Ref::new("CommaSegment"),
                        Ref::new("BinaryOperatorGrammar")
                    ]
                })
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "WhenClauseSegment".into(),
            NodeMatcher::new(
                SyntaxKind::WhenClause,
                Sequence::new(vec_of_erased![
                    Ref::keyword("WHEN"),
                    Sequence::new(vec_of_erased![
                        MetaSegment::implicit_indent(),
                        Ref::new("ExpressionSegment"),
                        MetaSegment::dedent(),
                    ]),
                    Conditional::new(MetaSegment::indent()).indented_then(),
                    Ref::keyword("THEN"),
                    Conditional::new(MetaSegment::implicit_indent()).indented_then_contents(),
                    Ref::new("ExpressionSegment"),
                    Conditional::new(MetaSegment::dedent()).indented_then_contents(),
                    Conditional::new(MetaSegment::dedent()).indented_then(),
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "ElseClauseSegment".into(),
            NodeMatcher::new(
                SyntaxKind::ElseClause,
                Sequence::new(vec![
                    Ref::keyword("ELSE").boxed(),
                    MetaSegment::implicit_indent().boxed(),
                    Ref::new("ExpressionSegment").boxed(),
                    MetaSegment::dedent().boxed(),
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "WhereClauseSegment".into(),
            NodeMatcher::new(
                SyntaxKind::WhereClause,
                Sequence::new(vec_of_erased![
                    Ref::keyword("WHERE"),
                    MetaSegment::implicit_indent(),
                    optionally_bracketed(vec_of_erased![Ref::new("ExpressionSegment")]),
                    MetaSegment::dedent()
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "SetOperatorSegment".into(),
            NodeMatcher::new(
                SyntaxKind::SetOperator,
                one_of(vec_of_erased![
                    Ref::new("UnionGrammar"),
                    Sequence::new(vec_of_erased![
                        one_of(vec_of_erased![Ref::keyword("INTERSECT"), Ref::keyword("EXCEPT")]),
                        Ref::keyword("ALL").optional(),
                    ]),
                    Ref::keyword("MINUS"),
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "ValuesClauseSegment".into(),
            NodeMatcher::new(
                SyntaxKind::ValuesClause,
                Sequence::new(vec![
                    one_of(vec![Ref::keyword("VALUE").boxed(), Ref::keyword("VALUES").boxed()])
                        .boxed(),
                    Delimited::new(vec![
                        Sequence::new(vec![
                            Ref::keyword("ROW").optional().boxed(),
                            Bracketed::new(vec![
                                Delimited::new(vec![
                                    Ref::keyword("DEFAULT").boxed(),
                                    Ref::new("LiteralGrammar").boxed(),
                                    Ref::new("ExpressionSegment").boxed(),
                                ])
                                .boxed(),
                            ])
                            .config(|this| this.parse_mode(ParseMode::Greedy))
                            .boxed(),
                        ])
                        .boxed(),
                    ])
                    .boxed(),
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "EmptyStructLiteralSegment".into(),
            NodeMatcher::new(
                SyntaxKind::EmptyStructLiteral,
                Sequence::new(vec![
                    Ref::new("StructTypeSegment").boxed(),
                    Ref::new("EmptyStructLiteralBracketsSegment").boxed(),
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "ObjectLiteralSegment".into(),
            NodeMatcher::new(
                SyntaxKind::ObjectLiteral,
                Bracketed::new(vec![
                    Delimited::new(vec![Ref::new("ObjectLiteralElementSegment").boxed()])
                        .config(|this| {
                            this.optional();
                        })
                        .boxed(),
                ])
                .config(|this| {
                    this.bracket_type("curly");
                })
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "ObjectLiteralElementSegment".into(),
            NodeMatcher::new(
                SyntaxKind::ObjectLiteralElement,
                Sequence::new(vec![
                    Ref::new("QuotedLiteralSegment").boxed(),
                    Ref::new("ColonSegment").boxed(),
                    Ref::new("BaseExpressionElementGrammar").boxed(),
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "TimeZoneGrammar".into(),
            NodeMatcher::new(
                SyntaxKind::TimeZoneGrammar,
                AnyNumberOf::new(vec![
                    Sequence::new(vec![
                        Ref::keyword("AT").boxed(),
                        Ref::keyword("TIME").boxed(),
                        Ref::keyword("ZONE").boxed(),
                        Ref::new("ExpressionSegment").boxed(),
                    ])
                    .boxed(),
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "BracketedArguments".into(),
            NodeMatcher::new(
                SyntaxKind::BracketedArguments,
                Bracketed::new(vec![
                    Delimited::new(vec![Ref::new("LiteralGrammar").boxed()])
                        .config(|this| {
                            this.optional();
                        })
                        .boxed(),
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "DatatypeSegment".into(),
            NodeMatcher::new(
                SyntaxKind::DataType,
                one_of(vec_of_erased![
                    Sequence::new(vec_of_erased![
                        one_of(vec_of_erased![Ref::keyword("TIME"), Ref::keyword("TIMESTAMP")]),
                        Bracketed::new(vec_of_erased![Ref::new("NumericLiteralSegment")])
                            .config(|this| this.optional()),
                        Sequence::new(vec_of_erased![
                            one_of(vec_of_erased![Ref::keyword("WITH"), Ref::keyword("WITHOUT")]),
                            Ref::keyword("TIME"),
                            Ref::keyword("ZONE"),
                        ])
                        .config(|this| this.optional()),
                    ]),
                    Sequence::new(vec_of_erased![
                        Ref::keyword("DOUBLE"),
                        Ref::keyword("PRECISION")
                    ]),
                    Sequence::new(vec_of_erased![
                        one_of(vec_of_erased![
                            Sequence::new(vec_of_erased![
                                one_of(vec_of_erased![
                                    Ref::keyword("CHARACTER"),
                                    Ref::keyword("BINARY")
                                ]),
                                one_of(vec_of_erased![
                                    Ref::keyword("VARYING"),
                                    Sequence::new(vec_of_erased![
                                        Ref::keyword("LARGE"),
                                        Ref::keyword("OBJECT"),
                                    ]),
                                ]),
                            ]),
                            Sequence::new(vec_of_erased![
                                Sequence::new(vec_of_erased![
                                    Ref::new("SingleIdentifierGrammar"),
                                    Ref::new("DotSegment"),
                                ])
                                .config(|this| this.optional()),
                                Ref::new("DatatypeIdentifierSegment"),
                            ]),
                        ]),
                        Ref::new("BracketedArguments").optional(),
                        one_of(vec_of_erased![
                            Ref::keyword("UNSIGNED"),
                            Ref::new("CharCharacterSetGrammar"),
                        ])
                        .config(|config| config.optional()),
                    ]),
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "AliasExpressionSegment".into(),
            NodeMatcher::new(
                SyntaxKind::AliasExpression,
                Sequence::new(vec_of_erased![
                    MetaSegment::indent(),
                    Ref::keyword("AS").optional(),
                    one_of(vec_of_erased![
                        Sequence::new(vec_of_erased![
                            Ref::new("SingleIdentifierGrammar"),
                            Bracketed::new(vec_of_erased![Ref::new("SingleIdentifierListSegment")])
                                .config(|this| this.optional())
                        ]),
                        Ref::new("SingleQuotedIdentifierSegment")
                    ]),
                    MetaSegment::dedent(),
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "ShorthandCastSegment".into(),
            NodeMatcher::new(
                SyntaxKind::CastExpression,
                Sequence::new(vec_of_erased![
                    one_of(vec_of_erased![
                        Ref::new("Expression_D_Grammar"),
                        Ref::new("CaseExpressionSegment")
                    ]),
                    AnyNumberOf::new(vec_of_erased![Sequence::new(vec_of_erased![
                        Ref::new("CastOperatorSegment"),
                        Ref::new("DatatypeSegment"),
                        Ref::new("TimeZoneGrammar").optional()
                    ]),])
                    .config(|this| this.min_times(1)),
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "ArrayAccessorSegment".into(),
            NodeMatcher::new(
                SyntaxKind::ArrayAccessor,
                Bracketed::new(vec![
                    Delimited::new(vec![
                        one_of(vec![
                            Ref::new("NumericLiteralSegment").boxed(),
                            Ref::new("ExpressionSegment").boxed(),
                        ])
                        .boxed(),
                    ])
                    .config(|this| this.delimiter(Ref::new("SliceSegment")))
                    .boxed(),
                ])
                .config(|this| {
                    this.bracket_type("square");
                    this.parse_mode(ParseMode::Greedy);
                })
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "ArrayLiteralSegment".into(),
            NodeMatcher::new(
                SyntaxKind::ArrayLiteral,
                Bracketed::new(vec_of_erased![
                    Delimited::new(vec_of_erased![Ref::new("BaseExpressionElementGrammar")])
                        .config(|this| {
                            this.delimiter(Ref::new("CommaSegment"));
                            this.optional();
                        }),
                ])
                .config(|this| {
                    this.bracket_type("square");
                    this.parse_mode(ParseMode::Greedy);
                })
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "TypedArrayLiteralSegment".into(),
            NodeMatcher::new(
                SyntaxKind::TypedArrayLiteral,
                Sequence::new(vec![
                    Ref::new("ArrayTypeSegment").boxed(),
                    Ref::new("ArrayLiteralSegment").boxed(),
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "StructTypeSegment".into(),
            NodeMatcher::new(SyntaxKind::StructType, Nothing::new().to_matchable())
                .to_matchable()
                .into(),
        ),
        (
            "StructLiteralSegment".into(),
            NodeMatcher::new(
                SyntaxKind::StructLiteral,
                Bracketed::new(vec_of_erased![Delimited::new(vec_of_erased![Sequence::new(
                    vec_of_erased![
                        Ref::new("BaseExpressionElementGrammar"),
                        Ref::new("AliasExpressionSegment").optional(),
                    ]
                )])])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "TypedStructLiteralSegment".into(),
            NodeMatcher::new(
                SyntaxKind::TypedStructLiteral,
                Sequence::new(vec![
                    Ref::new("StructTypeSegment").boxed(),
                    Ref::new("StructLiteralSegment").boxed(),
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "IntervalExpressionSegment".into(),
            NodeMatcher::new(
                SyntaxKind::IntervalExpression,
                Sequence::new(vec![
                    Ref::keyword("INTERVAL").boxed(),
                    one_of(vec![
                        Sequence::new(vec![
                            Ref::new("NumericLiteralSegment").boxed(),
                            one_of(vec![
                                Ref::new("QuotedLiteralSegment").boxed(),
                                Ref::new("DatetimeUnitSegment").boxed(),
                            ])
                            .boxed(),
                        ])
                        .boxed(),
                        Ref::new("QuotedLiteralSegment").boxed(),
                    ])
                    .boxed(),
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "ArrayTypeSegment".into(),
            NodeMatcher::new(SyntaxKind::ArrayType, Nothing::new().to_matchable())
                .to_matchable()
                .into(),
        ),
        (
            "SizedArrayTypeSegment".into(),
            NodeMatcher::new(
                SyntaxKind::SizedArrayType,
                Sequence::new(vec![
                    Ref::new("ArrayTypeSegment").boxed(),
                    Ref::new("ArrayAccessorSegment").boxed(),
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "UnorderedSelectStatementSegment".into(),
            NodeMatcher::new(
                SyntaxKind::SelectStatement,
                Sequence::new(vec_of_erased![
                    Ref::new("SelectClauseSegment"),
                    MetaSegment::dedent(),
                    Ref::new("FromClauseSegment").optional(),
                    Ref::new("WhereClauseSegment").optional(),
                    Ref::new("GroupByClauseSegment").optional(),
                    Ref::new("HavingClauseSegment").optional(),
                    Ref::new("OverlapsClauseSegment").optional(),
                    Ref::new("NamedWindowSegment").optional()
                ])
                .terminators(vec_of_erased![
                    Ref::new("SetOperatorSegment"),
                    Ref::new("WithNoSchemaBindingClauseSegment"),
                    Ref::new("WithDataClauseSegment"),
                    Ref::new("OrderByClauseSegment"),
                    Ref::new("LimitClauseSegment")
                ])
                .config(|this| {
                    this.parse_mode(ParseMode::GreedyOnceStarted);
                })
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "OverlapsClauseSegment".into(),
            NodeMatcher::new(
                SyntaxKind::OverlapsClause,
                Sequence::new(vec_of_erased![
                    Ref::keyword("OVERLAPS"),
                    one_of(vec_of_erased![
                        Bracketed::new(vec_of_erased![
                            Ref::new("DateTimeLiteralGrammar"),
                            Ref::new("CommaSegment"),
                            Ref::new("DateTimeLiteralGrammar"),
                        ]),
                        Ref::new("ColumnReferenceSegment"),
                    ]),
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        ("SelectClauseSegment".into(), {
            NodeMatcher::new(SyntaxKind::SelectClause, select_clause_segment())
                .to_matchable()
                .into()
        }),
        (
            "StatementSegment".into(),
            NodeMatcher::new(SyntaxKind::Statement, statement_segment()).to_matchable().into(),
        ),
        (
            "WithNoSchemaBindingClauseSegment".into(),
            NodeMatcher::new(
                SyntaxKind::WithNoSchemaBindingClause,
                Sequence::new(vec_of_erased![
                    Ref::keyword("WITH"),
                    Ref::keyword("NO"),
                    Ref::keyword("SCHEMA"),
                    Ref::keyword("BINDING"),
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "WithDataClauseSegment".into(),
            NodeMatcher::new(
                SyntaxKind::WithDataClause,
                Sequence::new(vec_of_erased![
                    Ref::keyword("WITH"),
                    Sequence::new(vec_of_erased![Ref::keyword("NO")])
                        .config(|this| this.optional()),
                    Ref::keyword("DATA"),
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "SetExpressionSegment".into(),
            NodeMatcher::new(
                SyntaxKind::SetExpression,
                Sequence::new(vec_of_erased![
                    Ref::new("NonSetSelectableGrammar"),
                    AnyNumberOf::new(vec_of_erased![Sequence::new(vec_of_erased![
                        Ref::new("SetOperatorSegment"),
                        Ref::new("NonSetSelectableGrammar"),
                    ])])
                    .config(|this| this.min_times(1)),
                    Ref::new("OrderByClauseSegment").optional(),
                    Ref::new("LimitClauseSegment").optional(),
                    Ref::new("NamedWindowSegment").optional(),
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "FromClauseSegment".into(),
            NodeMatcher::new(
                SyntaxKind::FromClause,
                Sequence::new(vec_of_erased![
                    Ref::keyword("FROM"),
                    Delimited::new(vec_of_erased![Ref::new("FromExpressionSegment")]),
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "EmptyStructLiteralBracketsSegment".into(),
            NodeMatcher::new(
                SyntaxKind::EmptyStructLiteralBrackets,
                Bracketed::new(vec![]).to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "WildcardExpressionSegment".into(),
            NodeMatcher::new(SyntaxKind::WildcardExpression, wildcard_expression_segment())
                .to_matchable()
                .into(),
        ),
        (
            "OrderByClauseSegment".into(),
            NodeMatcher::new(
                SyntaxKind::OrderbyClause,
                Sequence::new(vec_of_erased![
                    Ref::keyword("ORDER"),
                    Ref::keyword("BY"),
                    MetaSegment::indent(),
                    Delimited::new(vec_of_erased![Sequence::new(vec_of_erased![
                        one_of(vec_of_erased![
                            Ref::new("ColumnReferenceSegment"),
                            Ref::new("NumericLiteralSegment"),
                            Ref::new("ExpressionSegment"),
                        ]),
                        one_of(vec_of_erased![Ref::keyword("ASC"), Ref::keyword("DESC"),])
                            .config(|this| this.optional()),
                        Sequence::new(vec_of_erased![
                            Ref::keyword("NULLS"),
                            one_of(vec_of_erased![Ref::keyword("FIRST"), Ref::keyword("LAST"),]),
                        ])
                        .config(|this| this.optional()),
                    ])])
                    .config(|this| this.terminators =
                        vec_of_erased![Ref::keyword("LIMIT"), Ref::new("FrameClauseUnitGrammar")]),
                    MetaSegment::dedent(),
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "TruncateStatementSegment".into(),
            NodeMatcher::new(
                SyntaxKind::TruncateStatement,
                Sequence::new(vec![
                    Ref::keyword("TRUNCATE").boxed(),
                    Ref::keyword("TABLE").optional().boxed(),
                    Ref::new("TableReferenceSegment").boxed(),
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "FromExpressionSegment".into(),
            NodeMatcher::new(
                SyntaxKind::FromExpression,
                optionally_bracketed(vec_of_erased![Sequence::new(vec_of_erased![
                    MetaSegment::indent(),
                    one_of(vec_of_erased![
                        Ref::new("FromExpressionElementSegment"),
                        Bracketed::new(vec_of_erased![Ref::new("FromExpressionSegment")])
                    ])
                    .config(|this| this.terminators = vec_of_erased![
                        Sequence::new(vec_of_erased![Ref::keyword("ORDER"), Ref::keyword("BY")]),
                        Sequence::new(vec_of_erased![Ref::keyword("GROUP"), Ref::keyword("BY")]),
                    ]),
                    MetaSegment::dedent(),
                    Conditional::new(MetaSegment::indent()).indented_joins(),
                    AnyNumberOf::new(vec_of_erased![Sequence::new(vec_of_erased![
                        one_of(vec_of_erased![
                            Ref::new("JoinClauseSegment"),
                            Ref::new("JoinLikeClauseGrammar")
                        ])
                        .config(|this| {
                            this.optional();
                            this.terminators = vec_of_erased![
                                Sequence::new(vec_of_erased![
                                    Ref::keyword("ORDER"),
                                    Ref::keyword("BY")
                                ]),
                                Sequence::new(vec_of_erased![
                                    Ref::keyword("GROUP"),
                                    Ref::keyword("BY")
                                ]),
                            ];
                        })
                    ])]),
                    Conditional::new(MetaSegment::dedent()).indented_joins(),
                ])])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "DatePartFunctionNameSegment".into(),
            NodeMatcher::new(
                SyntaxKind::FunctionName,
                Ref::new("DatePartFunctionName").to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "FromExpressionElementSegment".into(),
            NodeMatcher::new(
                SyntaxKind::FromExpressionElement,
                Sequence::new(vec_of_erased![
                    Ref::new("PreTableFunctionKeywordsGrammar").optional(),
                    optionally_bracketed(vec_of_erased![Ref::new("TableExpressionSegment")]),
                    Ref::new("AliasExpressionSegment")
                        .exclude(one_of(vec_of_erased![
                            Ref::new("FromClauseTerminatorGrammar"),
                            Ref::new("SamplingExpressionSegment"),
                            Ref::new("JoinLikeClauseGrammar")
                        ]))
                        .optional(),
                    Sequence::new(vec_of_erased![
                        Ref::keyword("WITH"),
                        Ref::keyword("OFFSET"),
                        Ref::new("AliasExpressionSegment")
                    ])
                    .config(|this| this.optional()),
                    Ref::new("SamplingExpressionSegment").optional(),
                    Ref::new("PostTableExpressionGrammar").optional()
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "SelectStatementSegment".into(),
            NodeMatcher::new(SyntaxKind::SelectStatement, select_statement()).to_matchable().into(),
        ),
        (
            "CreateSchemaStatementSegment".into(),
            NodeMatcher::new(
                SyntaxKind::CreateSchemaStatement,
                Sequence::new(vec_of_erased![
                    Ref::keyword("CREATE"),
                    Ref::keyword("SCHEMA"),
                    Ref::new("IfNotExistsGrammar").optional(),
                    Ref::new("SchemaReferenceSegment")
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "SelectClauseModifierSegment".into(),
            NodeMatcher::new(
                SyntaxKind::SelectClauseModifier,
                one_of(vec![Ref::keyword("DISTINCT").boxed(), Ref::keyword("ALL").boxed()])
                    .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "SelectClauseElementSegment".into(),
            NodeMatcher::new(SyntaxKind::SelectClauseElement, select_clause_element())
                .to_matchable()
                .into(),
        ),
    ]);

    // hookpoint
    ansi_dialect.add([("CharCharacterSetGrammar".into(), Nothing::new().to_matchable().into())]);

    // This is a hook point to allow subclassing for other dialects
    ansi_dialect.add([(
        "AliasedTableReferenceGrammar".into(),
        Sequence::new(vec_of_erased![
            Ref::new("TableReferenceSegment"),
            Ref::new("AliasExpressionSegment"),
        ])
        .to_matchable()
        .into(),
    )]);

    ansi_dialect.add([
        // FunctionContentsExpressionGrammar intended as a hook to override in other dialects.
        (
            "FunctionContentsExpressionGrammar".into(),
            Ref::new("ExpressionSegment").to_matchable().into(),
        ),
        (
            "FunctionContentsGrammar".into(),
            AnyNumberOf::new(vec![
                Ref::new("ExpressionSegment").boxed(),
                // A Cast-like function
                Sequence::new(vec![
                    Ref::new("ExpressionSegment").boxed(),
                    Ref::keyword("AS").boxed(),
                    Ref::new("DatatypeSegment").boxed(),
                ])
                .boxed(),
                // Trim function
                Sequence::new(vec![
                    Ref::new("TrimParametersGrammar").boxed(),
                    Ref::new("ExpressionSegment").optional().exclude(Ref::keyword("FROM")).boxed(),
                    Ref::keyword("FROM").boxed(),
                    Ref::new("ExpressionSegment").boxed(),
                ])
                .boxed(),
                // An extract-like or substring-like function
                Sequence::new(vec![
                    one_of(vec![
                        Ref::new("DatetimeUnitSegment").boxed(),
                        Ref::new("ExpressionSegment").boxed(),
                    ])
                    .boxed(),
                    Ref::keyword("FROM").boxed(),
                    Ref::new("ExpressionSegment").boxed(),
                ])
                .boxed(),
                Sequence::new(vec![
                    // Allow an optional distinct keyword here.
                    Ref::keyword("DISTINCT").optional().boxed(),
                    one_of(vec![
                        // For COUNT(*) or similar
                        Ref::new("StarSegment").boxed(),
                        Delimited::new(vec![Ref::new("FunctionContentsExpressionGrammar").boxed()])
                            .boxed(),
                    ])
                    .boxed(),
                ])
                .boxed(),
                Ref::new("AggregateOrderByClause").boxed(), // Used in various functions
                Sequence::new(vec![
                    Ref::keyword("SEPARATOR").boxed(),
                    Ref::new("LiteralGrammar").boxed(),
                ])
                .boxed(),
                // Position-like function
                Sequence::new(vec![
                    one_of(vec![
                        Ref::new("QuotedLiteralSegment").boxed(),
                        Ref::new("SingleIdentifierGrammar").boxed(),
                        Ref::new("ColumnReferenceSegment").boxed(),
                    ])
                    .boxed(),
                    Ref::keyword("IN").boxed(),
                    one_of(vec![
                        Ref::new("QuotedLiteralSegment").boxed(),
                        Ref::new("SingleIdentifierGrammar").boxed(),
                        Ref::new("ColumnReferenceSegment").boxed(),
                    ])
                    .boxed(),
                ])
                .boxed(),
                Ref::new("IgnoreRespectNullsGrammar").boxed(),
                Ref::new("IndexColumnDefinitionSegment").boxed(),
                Ref::new("EmptyStructLiteralSegment").boxed(),
            ])
            .to_matchable()
            .into(),
        ),
        (
            "PostFunctionGrammar".into(),
            one_of(vec![
                Ref::new("OverClauseSegment").boxed(),
                Ref::new("FilterClauseGrammar").boxed(),
            ])
            .to_matchable()
            .into(),
        ),
    ]);

    // Assuming `ansi_dialect` is an instance of a struct representing a SQL dialect
    // and `add_grammar` is a method to add a new grammar rule to the dialect.
    ansi_dialect.add([("JoinLikeClauseGrammar".into(), Nothing::new().to_matchable().into())]);

    ansi_dialect.add([
        (
            // Expression_A_Grammar
            // https://www.cockroachlabs.com/docs/v20.2/sql-grammar.html#a_expr
            // The upstream grammar is defined recursively, which if implemented naively
            // will cause SQLFluff to overflow the stack from recursive function calls.
            // To work around this, the a_expr grammar is reworked a bit into sub-grammars
            // that effectively provide tail recursion.
            "Expression_A_Unary_Operator_Grammar".into(),
            one_of(vec![
                // This grammar corresponds to the unary operator portion of the initial
                // recursive block on the Cockroach Labs a_expr grammar.
                Ref::new("SignedSegmentGrammar")
                    .exclude(Sequence::new(vec![
                        Ref::new("QualifiedNumericLiteralSegment").boxed(),
                    ]))
                    .boxed(),
                Ref::new("TildeSegment").boxed(),
                Ref::new("NotOperatorGrammar").boxed(),
                // Used in CONNECT BY clauses (EXASOL, Snowflake, Postgres...)
                Ref::keyword("PRIOR").boxed(),
            ])
            .to_matchable()
            .into(),
        ),
        (
            "Tail_Recurse_Expression_A_Grammar".into(),
            Sequence::new(vec_of_erased![
                // This should be used instead of a recursive call to Expression_A_Grammar
                // whenever the repeating element in Expression_A_Grammar makes a recursive
                // call to itself at the _end_.
                AnyNumberOf::new(vec_of_erased![Ref::new("Expression_A_Unary_Operator_Grammar")])
                    .config(
                        |this| this.terminators = vec_of_erased![Ref::new("BinaryOperatorGrammar")]
                    ),
                Ref::new("Expression_C_Grammar"),
            ])
            .to_matchable()
            .into(),
        ),
        (
            "Expression_A_Grammar".into(),
            Sequence::new(vec![
                Ref::new("Tail_Recurse_Expression_A_Grammar").boxed(),
                AnyNumberOf::new(vec![
                    one_of(vec![
                        // Like grammar with NOT and optional ESCAPE
                        Sequence::new(vec![
                            Sequence::new(vec![
                                Ref::keyword("NOT").optional().boxed(),
                                Ref::new("LikeGrammar").boxed(),
                            ])
                            .boxed(),
                            Ref::new("Expression_A_Grammar").boxed(),
                            Sequence::new(vec![
                                Ref::keyword("ESCAPE").boxed(),
                                Ref::new("Tail_Recurse_Expression_A_Grammar").boxed(),
                            ])
                            .config(|this| this.optional())
                            .boxed(),
                        ])
                        .boxed(),
                        // Binary operator grammar
                        Sequence::new(vec![
                            Ref::new("BinaryOperatorGrammar").boxed(),
                            Ref::new("Tail_Recurse_Expression_A_Grammar").boxed(),
                        ])
                        .boxed(),
                        // IN grammar with NOT and brackets
                        Sequence::new(vec![
                            Ref::keyword("NOT").optional().boxed(),
                            Ref::keyword("IN").boxed(),
                            Bracketed::new(vec![
                                one_of(vec![
                                    Delimited::new(vec![Ref::new("Expression_A_Grammar").boxed()])
                                        .boxed(),
                                    Ref::new("SelectableGrammar").boxed(),
                                ])
                                .boxed(),
                            ])
                            .config(|this| this.parse_mode(ParseMode::Greedy))
                            .boxed(),
                        ])
                        .boxed(),
                        // IN grammar with function segment
                        Sequence::new(vec![
                            Ref::keyword("NOT").optional().boxed(),
                            Ref::keyword("IN").boxed(),
                            Ref::new("FunctionSegment").boxed(),
                        ])
                        .boxed(),
                        // IS grammar
                        Sequence::new(vec![
                            Ref::keyword("IS").boxed(),
                            Ref::keyword("NOT").optional().boxed(),
                            Ref::new("IsClauseGrammar").boxed(),
                        ])
                        .boxed(),
                        // IS NULL and NOT NULL grammars
                        Ref::new("IsNullGrammar").boxed(),
                        Ref::new("NotNullGrammar").boxed(),
                        // COLLATE grammar
                        Ref::new("CollateGrammar").boxed(),
                        // BETWEEN grammar
                        Sequence::new(vec![
                            Ref::keyword("NOT").optional().boxed(),
                            Ref::keyword("BETWEEN").boxed(),
                            Ref::new("Expression_B_Grammar").boxed(),
                            Ref::keyword("AND").boxed(),
                            Ref::new("Tail_Recurse_Expression_A_Grammar").boxed(),
                        ])
                        .boxed(),
                        // Additional sequences and grammar rules can be added here
                    ])
                    .boxed(),
                ])
                .boxed(),
            ])
            .to_matchable()
            .into(),
        ),
        // Expression_B_Grammar: Does not directly feed into Expression_A_Grammar
        // but is used for a BETWEEN statement within Expression_A_Grammar.
        // https://www.cockroachlabs.com/docs/v20.2/sql-grammar.htm#b_expr
        // We use a similar trick as seen with Expression_A_Grammar to avoid recursion
        // by using a tail recursion grammar.  See the comments for a_expr to see how
        // that works.
        (
            "Expression_B_Unary_Operator_Grammar".into(),
            one_of(vec![
                Ref::new("SignedSegmentGrammar")
                    .exclude(Sequence::new(vec![
                        Ref::new("QualifiedNumericLiteralSegment").boxed(),
                    ]))
                    .boxed(),
                Ref::new("TildeSegment").boxed(),
            ])
            .to_matchable()
            .into(),
        ),
        (
            "Tail_Recurse_Expression_B_Grammar".into(),
            Sequence::new(vec![
                // Only safe to use if the recursive call is at the END of the repeating
                // element in the main b_expr portion.
                AnyNumberOf::new(vec![Ref::new("Expression_B_Unary_Operator_Grammar").boxed()])
                    .boxed(),
                Ref::new("Expression_C_Grammar").boxed(),
            ])
            .to_matchable()
            .into(),
        ),
        (
            "Expression_B_Grammar".into(),
            Sequence::new(vec![
                // Always start with the tail recursion element
                Ref::new("Tail_Recurse_Expression_B_Grammar").boxed(),
                AnyNumberOf::new(vec![
                    one_of(vec![
                        // Arithmetic, string, or comparison binary operators followed by tail
                        // recursion
                        Sequence::new(vec![
                            one_of(vec![
                                Ref::new("ArithmeticBinaryOperatorGrammar").boxed(),
                                Ref::new("StringBinaryOperatorGrammar").boxed(),
                                Ref::new("ComparisonOperatorGrammar").boxed(),
                            ])
                            .boxed(),
                            Ref::new("Tail_Recurse_Expression_B_Grammar").boxed(),
                        ])
                        .boxed(),
                        // Additional sequences and rules from b_expr can be added here
                    ])
                    .boxed(),
                ])
                .boxed(),
            ])
            .to_matchable()
            .into(),
        ),
        (
            "Expression_C_Grammar".into(),
            one_of(vec![
                // Sequence for "EXISTS" with a bracketed selectable grammar
                Sequence::new(vec![
                    Ref::keyword("EXISTS").boxed(),
                    Bracketed::new(vec![Ref::new("SelectableGrammar").boxed()]).boxed(),
                ])
                .boxed(),
                // Sequence for Expression_D_Grammar or CaseExpressionSegment
                // followed by any number of TimeZoneGrammar
                Sequence::new(vec![
                    one_of(vec![
                        Ref::new("Expression_D_Grammar").boxed(),
                        Ref::new("CaseExpressionSegment").boxed(),
                    ])
                    .boxed(),
                    AnyNumberOf::new(vec![Ref::new("TimeZoneGrammar").boxed()])
                        .config(|this| this.optional())
                        .boxed(),
                ])
                .boxed(),
                Ref::new("ShorthandCastSegment").boxed(),
            ])
            .config(|this| this.terminators = vec_of_erased![Ref::new("CommaSegment")])
            .to_matchable()
            .into(),
        ),
        (
            "Expression_D_Grammar".into(),
            Sequence::new(vec![
                one_of(vec![
                    Ref::new("BareFunctionSegment").boxed(),
                    Ref::new("FunctionSegment").boxed(),
                    Bracketed::new(vec![
                        one_of(vec![
                            Ref::new("ExpressionSegment").boxed(),
                            Ref::new("SelectableGrammar").boxed(),
                            Delimited::new(vec![
                                Ref::new("ColumnReferenceSegment").boxed(),
                                Ref::new("FunctionSegment").boxed(),
                                Ref::new("LiteralGrammar").boxed(),
                                Ref::new("LocalAliasSegment").boxed(),
                            ])
                            .boxed(),
                        ])
                        .boxed(),
                    ])
                    .config(|this| this.parse_mode(ParseMode::Greedy))
                    .boxed(),
                    Ref::new("SelectStatementSegment").boxed(),
                    Ref::new("LiteralGrammar").boxed(),
                    Ref::new("IntervalExpressionSegment").boxed(),
                    Ref::new("TypedStructLiteralSegment").boxed(),
                    Ref::new("ArrayExpressionSegment").boxed(),
                    Ref::new("ColumnReferenceSegment").boxed(),
                    Sequence::new(vec![
                        Ref::new("SingleIdentifierGrammar").boxed(),
                        Ref::new("ObjectReferenceDelimiterGrammar").boxed(),
                        Ref::new("StarSegment").boxed(),
                    ])
                    .boxed(),
                    Sequence::new(vec![
                        Ref::new("StructTypeSegment").boxed(),
                        Bracketed::new(vec![
                            Delimited::new(vec![Ref::new("ExpressionSegment").boxed()]).boxed(),
                        ])
                        .boxed(),
                    ])
                    .boxed(),
                    Sequence::new(vec![
                        Ref::new("DatatypeSegment").boxed(),
                        one_of(vec![
                            Ref::new("QuotedLiteralSegment").boxed(),
                            Ref::new("NumericLiteralSegment").boxed(),
                            Ref::new("BooleanLiteralGrammar").boxed(),
                            Ref::new("NullLiteralSegment").boxed(),
                            Ref::new("DateTimeLiteralGrammar").boxed(),
                        ])
                        .boxed(),
                    ])
                    .boxed(),
                    Ref::new("LocalAliasSegment").boxed(),
                ])
                .config(|this| this.terminators = vec_of_erased![Ref::new("CommaSegment")])
                .boxed(),
                Ref::new("AccessorGrammar").optional().boxed(),
            ])
            .allow_gaps(true)
            .to_matchable()
            .into(),
        ),
        (
            "AccessorGrammar".into(),
            AnyNumberOf::new(vec![Ref::new("ArrayAccessorSegment").boxed()]).to_matchable().into(),
        ),
    ]);

    ansi_dialect.add([
        (
            "SelectableGrammar".into(),
            one_of(vec![
                optionally_bracketed(vec![Ref::new("WithCompoundStatementSegment").boxed()])
                    .boxed(),
                Ref::new("NonWithSelectableGrammar").boxed(),
                Bracketed::new(vec![Ref::new("SelectableGrammar").boxed()]).boxed(),
            ])
            .to_matchable()
            .into(),
        ),
        (
            "NonWithSelectableGrammar".into(),
            one_of(vec![
                Ref::new("SetExpressionSegment").boxed(),
                optionally_bracketed(vec![Ref::new("SelectStatementSegment").boxed()]).boxed(),
                Ref::new("NonSetSelectableGrammar").boxed(),
            ])
            .to_matchable()
            .into(),
        ),
        (
            "NonWithNonSelectableGrammar".into(),
            one_of(vec![
                Ref::new("UpdateStatementSegment").boxed(),
                Ref::new("InsertStatementSegment").boxed(),
                Ref::new("DeleteStatementSegment").boxed(),
            ])
            .to_matchable()
            .into(),
        ),
        (
            "NonSetSelectableGrammar".into(),
            one_of(vec![
                Ref::new("ValuesClauseSegment").boxed(),
                Ref::new("UnorderedSelectStatementSegment").boxed(),
                Bracketed::new(vec![Ref::new("SelectStatementSegment").boxed()]).boxed(),
                Bracketed::new(vec![Ref::new("NonSetSelectableGrammar").boxed()]).boxed(),
            ])
            .to_matchable()
            .into(),
        ),
    ]);

    // This is a hook point to allow subclassing for other dialects
    ansi_dialect.add([
        ("PostTableExpressionGrammar".into(), Nothing::new().to_matchable().into()),
        ("BracketedSegment".into(), BracketedSegmentMatcher::new().to_matchable().into()),
    ]);

    ansi_dialect
}

pub fn select_clause_element() -> Arc<dyn Matchable> {
    one_of(vec_of_erased![
        // *, blah.*, blah.blah.*, etc.
        Ref::new("WildcardExpressionSegment"),
        Sequence::new(vec_of_erased![
            Ref::new("BaseExpressionElementGrammar"),
            Ref::new("AliasExpressionSegment").optional(),
        ]),
    ])
    .to_matchable()
}

fn lexer_matchers() -> Vec<Matcher> {
    vec![
        Matcher::regex("whitespace", r"[^\S\r\n]+", SyntaxKind::Whitespace),
        Matcher::regex("inline_comment", r"(--|#)[^\n]*", SyntaxKind::InlineComment),
        Matcher::regex("block_comment", r"\/\*([^\*]|\*(?!\/))*\*\/", SyntaxKind::BlockComment)
            .subdivider(Pattern::regex("newline", r"\r\n|\n", SyntaxKind::Newline))
            .post_subdivide(Pattern::regex("whitespace", r"[^\S\r\n]+", SyntaxKind::Whitespace)),
        Matcher::regex("single_quote", r"'([^'\\]|\\.|'')*'", SyntaxKind::SingleQuote),
        Matcher::regex("double_quote", r#""([^"\\]|\\.)*""#, SyntaxKind::DoubleQuote),
        Matcher::regex("back_quote", r"`[^`]*`", SyntaxKind::BackQuote),
        Matcher::regex("dollar_quote", r"\$(\w*)\$[\s\S]*?\$\1\$", SyntaxKind::DollarQuote),
        Matcher::regex(
            "numeric_literal",
            r"(?>\d+\.\d+|\d+\.(?![\.\w])|\.\d+|\d+)(\.?[eE][+-]?\d+)?((?<=\.)|(?=\b))",
            SyntaxKind::NumericLiteral,
        ),
        Matcher::regex("like_operator", r"!?~~?\*?", SyntaxKind::LikeOperator),
        Matcher::regex("newline", r"\r\n|\n", SyntaxKind::Newline),
        Matcher::string("casting_operator", "::", SyntaxKind::CastingOperator),
        Matcher::string("equals", "=", SyntaxKind::RawComparisonOperator),
        Matcher::string("greater_than", ">", SyntaxKind::RawComparisonOperator),
        Matcher::string("less_than", "<", SyntaxKind::RawComparisonOperator),
        Matcher::string("not", "!", SyntaxKind::RawComparisonOperator),
        Matcher::string("dot", ".", SyntaxKind::Dot),
        Matcher::string("comma", ",", SyntaxKind::Comma),
        Matcher::string("plus", "+", SyntaxKind::Plus),
        Matcher::string("minus", "-", SyntaxKind::Minus),
        Matcher::string("divide", "/", SyntaxKind::Divide),
        Matcher::string("percent", "%", SyntaxKind::Percent),
        Matcher::string("question", "?", SyntaxKind::Question),
        Matcher::string("ampersand", "&", SyntaxKind::Ampersand),
        Matcher::string("vertical_bar", "|", SyntaxKind::VerticalBar),
        Matcher::string("caret", "^", SyntaxKind::Caret),
        Matcher::string("star", "*", SyntaxKind::Star),
        Matcher::string("start_bracket", "(", SyntaxKind::StartBracket),
        Matcher::string("end_bracket", ")", SyntaxKind::EndBracket),
        Matcher::string("start_square_bracket", "[", SyntaxKind::StartSquareBracket),
        Matcher::string("end_square_bracket", "]", SyntaxKind::EndSquareBracket),
        Matcher::string("start_curly_bracket", "{", SyntaxKind::StartCurlyBracket),
        Matcher::string("end_curly_bracket", "}", SyntaxKind::EndCurlyBracket),
        Matcher::string("colon", ":", SyntaxKind::Colon),
        Matcher::string("semicolon", ";", SyntaxKind::Semicolon),
        Matcher::regex("word", "[0-9a-zA-Z_]+", SyntaxKind::Word),
    ]
}

pub fn frame_extent() -> AnyNumberOf {
    one_of(vec_of_erased![
        Sequence::new(vec_of_erased![Ref::keyword("CURRENT"), Ref::keyword("ROW")]),
        Sequence::new(vec_of_erased![
            one_of(vec_of_erased![
                Ref::new("NumericLiteralSegment"),
                Sequence::new(vec_of_erased![
                    Ref::keyword("INTERVAL"),
                    Ref::new("QuotedLiteralSegment")
                ]),
                Ref::keyword("UNBOUNDED")
            ]),
            one_of(vec_of_erased![Ref::keyword("PRECEDING"), Ref::keyword("FOLLOWING")])
        ])
    ])
}

pub fn explainable_stmt() -> AnyNumberOf {
    one_of(vec_of_erased![
        Ref::new("SelectableGrammar"),
        Ref::new("InsertStatementSegment"),
        Ref::new("UpdateStatementSegment"),
        Ref::new("DeleteStatementSegment")
    ])
}

pub fn get_unordered_select_statement_segment_grammar() -> Arc<dyn Matchable> {
    Sequence::new(vec_of_erased![
        Ref::new("SelectClauseSegment"),
        MetaSegment::dedent(),
        Ref::new("FromClauseSegment").optional(),
        Ref::new("WhereClauseSegment").optional(),
        Ref::new("GroupByClauseSegment").optional(),
        Ref::new("HavingClauseSegment").optional(),
        Ref::new("OverlapsClauseSegment").optional(),
        Ref::new("NamedWindowSegment").optional()
    ])
    .terminators(vec_of_erased![
        Ref::new("SetOperatorSegment"),
        Ref::new("WithNoSchemaBindingClauseSegment"),
        Ref::new("WithDataClauseSegment"),
        Ref::new("OrderByClauseSegment"),
        Ref::new("LimitClauseSegment")
    ])
    .config(|this| {
        this.parse_mode(ParseMode::GreedyOnceStarted);
    })
    .to_matchable()
}

pub fn select_statement() -> Arc<dyn Matchable> {
    get_unordered_select_statement_segment_grammar().copy(
        Some(vec_of_erased![
            Ref::new("OrderByClauseSegment").optional(),
            Ref::new("FetchClauseSegment").optional(),
            Ref::new("LimitClauseSegment").optional(),
            Ref::new("NamedWindowSegment").optional()
        ]),
        None,
        None,
        None,
        vec_of_erased![
            Ref::new("SetOperatorSegment"),
            Ref::new("WithNoSchemaBindingClauseSegment"),
            Ref::new("WithDataClauseSegment")
        ],
        true,
    )
}

pub fn select_clause_segment() -> Arc<dyn Matchable> {
    Sequence::new(vec_of_erased![
        Ref::keyword("SELECT"),
        Ref::new("SelectClauseModifierSegment").optional(),
        MetaSegment::indent(),
        Delimited::new(vec_of_erased![Ref::new("SelectClauseElementSegment")])
            .config(|this| this.allow_trailing()),
    ])
    .terminators(vec_of_erased![Ref::new("SelectClauseTerminatorGrammar")])
    .config(|this| {
        this.parse_mode(ParseMode::GreedyOnceStarted);
    })
    .to_matchable()
}

pub fn statement_segment() -> Arc<dyn Matchable> {
    one_of(vec![
        Ref::new("SelectableGrammar").boxed(),
        Ref::new("MergeStatementSegment").boxed(),
        Ref::new("InsertStatementSegment").boxed(),
        Ref::new("TransactionStatementSegment").boxed(),
        Ref::new("DropTableStatementSegment").boxed(),
        Ref::new("DropViewStatementSegment").boxed(),
        Ref::new("CreateUserStatementSegment").boxed(),
        Ref::new("DropUserStatementSegment").boxed(),
        Ref::new("TruncateStatementSegment").boxed(),
        Ref::new("AccessStatementSegment").boxed(),
        Ref::new("CreateTableStatementSegment").boxed(),
        Ref::new("CreateRoleStatementSegment").boxed(),
        Ref::new("DropRoleStatementSegment").boxed(),
        Ref::new("AlterTableStatementSegment").boxed(),
        Ref::new("CreateSchemaStatementSegment").boxed(),
        Ref::new("SetSchemaStatementSegment").boxed(),
        Ref::new("DropSchemaStatementSegment").boxed(),
        Ref::new("DropTypeStatementSegment").boxed(),
        Ref::new("CreateDatabaseStatementSegment").boxed(),
        Ref::new("DropDatabaseStatementSegment").boxed(),
        Ref::new("CreateIndexStatementSegment").boxed(),
        Ref::new("DropIndexStatementSegment").boxed(),
        Ref::new("CreateViewStatementSegment").boxed(),
        Ref::new("DeleteStatementSegment").boxed(),
        Ref::new("UpdateStatementSegment").boxed(),
        Ref::new("CreateCastStatementSegment").boxed(),
        Ref::new("DropCastStatementSegment").boxed(),
        Ref::new("CreateFunctionStatementSegment").boxed(),
        Ref::new("DropFunctionStatementSegment").boxed(),
        Ref::new("CreateModelStatementSegment").boxed(),
        Ref::new("DropModelStatementSegment").boxed(),
        Ref::new("DescribeStatementSegment").boxed(),
        Ref::new("UseStatementSegment").boxed(),
        Ref::new("ExplainStatementSegment").boxed(),
        Ref::new("CreateSequenceStatementSegment").boxed(),
        Ref::new("AlterSequenceStatementSegment").boxed(),
        Ref::new("DropSequenceStatementSegment").boxed(),
        Ref::new("CreateTriggerStatementSegment").boxed(),
        Ref::new("DropTriggerStatementSegment").boxed(),
    ])
    .config(|this| this.terminators = vec_of_erased![Ref::new("DelimiterGrammar")])
    .to_matchable()
}

pub fn wildcard_expression_segment() -> Arc<dyn Matchable> {
    Sequence::new(vec![Ref::new("WildcardIdentifierSegment").boxed()]).to_matchable()
}
