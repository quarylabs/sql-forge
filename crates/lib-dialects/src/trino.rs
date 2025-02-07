use sqruff_lib_core::dialects::base::Dialect;
use sqruff_lib_core::dialects::init::DialectKind;
use sqruff_lib_core::dialects::syntax::SyntaxKind;
use sqruff_lib_core::helpers::{Config, ToMatchable};
use sqruff_lib_core::parser::grammar::anyof::{one_of, AnyNumberOf};
use sqruff_lib_core::parser::grammar::base::{Anything, Nothing, Ref};
use sqruff_lib_core::parser::grammar::delimited::Delimited;
use sqruff_lib_core::parser::grammar::sequence::{Bracketed, Sequence};
use sqruff_lib_core::parser::matchable::MatchableTrait;
use sqruff_lib_core::parser::node_matcher::NodeMatcher;
use sqruff_lib_core::parser::parsers::TypedParser;
use sqruff_lib_core::parser::segments::meta::MetaSegment;
use sqruff_lib_core::vec_of_erased;

pub fn dialect() -> Dialect {
    let ansi_dialect = super::ansi::raw_dialect();
    let mut trino_dialect = ansi_dialect;
    trino_dialect.name = DialectKind::Trino;

    trino_dialect.sets_mut("bare_functions").extend([
        "current_date",
        "current_time",
        "current_timestamp",
        "localtime",
        "localtimestamp",
    ]);

    trino_dialect.sets_mut("unreserved_keywords").clear();
    trino_dialect.update_keywords_set_from_multiline_string(
        "unreserved_keywords",
        super::trino_keywords::TRINO_UNRESERVED_KEYWORDS,
    );

    trino_dialect.sets_mut("reserved_keywords").clear();
    trino_dialect.update_keywords_set_from_multiline_string(
        "reserved_keywords",
        super::trino_keywords::TRINO_RESERVED_KEYWORDS,
    );

    trino_dialect.add([
        (
            "DateTimeLiteralGrammar".into(),
            one_of(vec_of_erased![
                Sequence::new(vec_of_erased![
                    one_of(vec_of_erased![
                        Ref::keyword("DATE"),
                        Ref::keyword("TIME"),
                        Ref::keyword("TIMESTAMP")
                    ]),
                    TypedParser::new(SyntaxKind::SingleQuote, SyntaxKind::DateConstructorLiteral)
                ]),
                Ref::new("IntervalExpressionSegment")
            ])
            .to_matchable()
            .into(),
        ),
        (
            "LikeGrammar".into(),
            Sequence::new(vec_of_erased![Ref::keyword("LIKE")])
                .to_matchable()
                .into(),
        ),
        (
            "MLTableExpressionSegment".into(),
            Nothing::new().to_matchable().into(),
        ),
        (
            "FromClauseTerminatorGrammar".into(),
            one_of(vec_of_erased![
                Ref::keyword("WHERE"),
                Ref::keyword("LIMIT"),
                Sequence::new(vec_of_erased![Ref::keyword("GROUP"), Ref::keyword("BY")]),
                Sequence::new(vec_of_erased![Ref::keyword("ORDER"), Ref::keyword("BY")]),
                Ref::keyword("HAVING"),
                Ref::keyword("WINDOW"),
                Ref::new("SetOperatorSegment"),
                Ref::new("WithNoSchemaBindingClauseSegment"),
                Ref::new("WithDataClauseSegment"),
                Ref::keyword("FETCH")
            ])
            .to_matchable()
            .into(),
        ),
        (
            "OrderByClauseTerminators".into(),
            one_of(vec_of_erased![
                Ref::keyword("LIMIT"),
                Ref::keyword("HAVING"),
                Ref::keyword("WINDOW"),
                Ref::new("FrameClauseUnitGrammar"),
                Ref::keyword("FETCH")
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
                Ref::new("SetOperatorSegment"),
                Ref::keyword("FETCH")
            ])
            .to_matchable()
            .into(),
        ),
        (
            "WhereClauseTerminatorGrammar".into(),
            one_of(vec_of_erased![
                Ref::keyword("LIMIT"),
                Sequence::new(vec_of_erased![Ref::keyword("GROUP"), Ref::keyword("BY")]),
                Sequence::new(vec_of_erased![Ref::keyword("ORDER"), Ref::keyword("BY")]),
                Ref::keyword("HAVING"),
                Ref::keyword("WINDOW"),
                Ref::keyword("FETCH")
            ])
            .to_matchable()
            .into(),
        ),
        (
            "HavingClauseTerminatorGrammar".into(),
            one_of(vec_of_erased![
                Sequence::new(vec_of_erased![Ref::keyword("ORDER"), Ref::keyword("BY")]),
                Ref::keyword("LIMIT"),
                Ref::keyword("WINDOW"),
                Ref::keyword("FETCH")
            ])
            .to_matchable()
            .into(),
        ),
        (
            "GroupByClauseTerminatorGrammar".into(),
            one_of(vec_of_erased![
                Sequence::new(vec_of_erased![Ref::keyword("ORDER"), Ref::keyword("BY")]),
                Ref::keyword("LIMIT"),
                Ref::keyword("HAVING"),
                Ref::keyword("WINDOW"),
                Ref::keyword("FETCH")
            ])
            .to_matchable()
            .into(),
        ),
        (
            "Expression_A_Unary_Operator_Grammar".into(),
            one_of(vec_of_erased![
                Ref::new("SignedSegmentGrammar").exclude(Sequence::new(vec_of_erased![Ref::new(
                    "QualifiedNumericLiteralSegment"
                )])),
                Ref::new("TildeSegment"),
                Ref::new("NotOperatorGrammar")
            ])
            .to_matchable()
            .into(),
        ),
        (
            "FunctionContentsGrammar".into(),
            AnyNumberOf::new(vec_of_erased![
                Ref::new("ExpressionSegment"),
                // A Cast-like function
                Sequence::new(vec_of_erased![
                    Ref::new("ExpressionSegment"),
                    Ref::keyword("AS"),
                    Ref::new("DatatypeSegment")
                ]),
                // A Trim function
                Sequence::new(vec_of_erased![
                    Ref::new("TrimParametersGrammar"),
                    Ref::new("ExpressionSegment")
                        .optional()
                        .exclude(Ref::keyword("FROM")),
                    Ref::keyword("FROM"),
                    Ref::new("ExpressionSegment")
                ]),
                // An extract-like or substring-like function
                Sequence::new(vec_of_erased![
                    one_of(vec_of_erased![
                        Ref::new("DatetimeUnitSegment"),
                        Ref::new("ExpressionSegment")
                    ]),
                    Ref::keyword("FROM"),
                    Ref::new("ExpressionSegment")
                ]),
                Sequence::new(vec_of_erased![
                    Ref::keyword("DISTINCT").optional(),
                    one_of(vec_of_erased![
                        Ref::new("StarSegment"),
                        Delimited::new(vec_of_erased![Ref::new(
                            "FunctionContentsExpressionGrammar"
                        )])
                    ])
                ]),
                Ref::new("OrderByClauseSegment"),
                Sequence::new(vec_of_erased![
                    one_of(vec_of_erased![
                        Ref::new("QuotedLiteralSegment"),
                        Ref::new("SingleIdentifierGrammar"),
                        Ref::new("ColumnReferenceSegment")
                    ]),
                    Ref::keyword("IN"),
                    one_of(vec_of_erased![
                        Ref::new("QuotedLiteralSegment"),
                        Ref::new("SingleIdentifierGrammar"),
                        Ref::new("ColumnReferenceSegment")
                    ])
                ]),
                Ref::new("IgnoreRespectNullsGrammar"),
                Ref::new("IndexColumnDefinitionSegment"),
                Ref::new("EmptyStructLiteralSegment"),
                Ref::new("ListaggOverflowClauseSegment")
            ])
            .to_matchable()
            .into(),
        ),
    ]);
    trino_dialect.replace_grammar(
        "UnorderedSelectStatementSegment",
        Sequence::new(vec_of_erased![
            Ref::new("SelectClauseSegment"),
            MetaSegment::dedent(),
            Ref::new("FromClauseSegment").optional(),
            Ref::new("WhereClauseSegment").optional(),
            Ref::new("GroupByClauseSegment").optional(),
            Ref::new("HavingClauseSegment").optional(),
            Ref::new("NamedWindowSegment").optional()
        ])
        .to_matchable(),
    );

    trino_dialect.add([
        (
            "DatatypeSegment".into(),
            NodeMatcher::new(
                SyntaxKind::DataType,
                one_of(vec_of_erased![
                    Ref::keyword("BOOLEAN"),
                    Ref::keyword("TINYINT"),
                    Ref::keyword("SMALLINT"),
                    Ref::keyword("INTEGER"),
                    Ref::keyword("BIGINT"),
                    Ref::keyword("REAL"),
                    Ref::keyword("DOUBLE"),
                    Sequence::new(vec_of_erased![
                        Ref::keyword("DECIMAL"),
                        Ref::new("BracketedArguments").optional()
                    ]),
                    Sequence::new(vec_of_erased![
                        one_of(vec_of_erased![
                            Ref::keyword("CHAR"),
                            Ref::keyword("VARCHAR")
                        ]),
                        Ref::new("BracketedArguments").optional()
                    ]),
                    Ref::keyword("VARBINARY"),
                    Ref::keyword("JSON"),
                    Ref::keyword("DATE"),
                    Sequence::new(vec_of_erased![
                        one_of(vec_of_erased![
                            Ref::keyword("TIME"),
                            Ref::keyword("TIMESTAMP")
                        ]),
                        Bracketed::new(vec_of_erased![Ref::new("NumericLiteralSegment")]).config(
                            |config| {
                                config.optional();
                            }
                        ),
                        Sequence::new(vec_of_erased![
                            one_of(vec_of_erased![
                                Ref::keyword("WITH"),
                                Ref::keyword("WITHOUT")
                            ]),
                            Ref::keyword("TIME"),
                            Ref::keyword("ZONE")
                        ])
                        .config(|config| {
                            config.optional();
                        })
                    ]),
                    Ref::keyword("ARRAY"),
                    Ref::keyword("MAP"),
                    Ref::keyword("ROW"),
                    Ref::keyword("IPADDRESS"),
                    Ref::keyword("UUID")
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "OverlapsClauseSegment".into(),
            NodeMatcher::new(SyntaxKind::OverlapsClause, Nothing::new().to_matchable())
                .to_matchable()
                .into(),
        ),
    ]);

    trino_dialect.replace_grammar(
        "ValuesClauseSegment",
        Sequence::new(vec_of_erased![
            Ref::keyword("VALUES"),
            Delimited::new(vec_of_erased![Ref::new("ExpressionSegment")])
        ])
        .to_matchable(),
    );

    trino_dialect.replace_grammar(
        "StatementSegment",
        super::ansi::statement_segment().copy(
            Some(vec_of_erased![
                Ref::new("AnalyzeStatementSegment"),
                Ref::new("CommentOnStatementSegment")
            ]),
            None,
            None,
            Some(vec_of_erased![Ref::new("TransactionStatementSegment")]),
            Vec::new(),
            false,
        ),
    );

    trino_dialect.add([
        // An 'ANALYZE' statement.
        // As per docs https://trino.io/docs/current/sql/analyze.html
        (
            "AnalyzeStatementSegment".into(),
            Sequence::new(vec_of_erased![
                Ref::keyword("ANALYZE"),
                Ref::new("TableReferenceSegment"),
                Sequence::new(vec_of_erased![
                    Ref::keyword("WITH"),
                    Bracketed::new(vec_of_erased![Delimited::new(vec_of_erased![
                        Ref::new("ParameterNameSegment"),
                        Ref::new("EqualsSegment"),
                        Ref::new("ExpressionSegment"),
                    ]),]),
                ])
                .config(|config| {
                    config.optional();
                })
            ])
            .to_matchable()
            .into(),
        ),
        (
            "PostFunctionGrammar".into(),
            super::ansi::raw_dialect()
                .grammar("PostFunctionGrammar")
                .copy(
                    Some(vec_of_erased![Ref::new("WithinGroupClauseSegment")]),
                    None,
                    None,
                    Some(vec_of_erased![Ref::new("TransactionStatementSegment")]),
                    Vec::new(),
                    false,
                )
                .into(),
        ),
        (
            // ON OVERFLOW clause of listagg function.
            // https://trino.io/docs/current/functions/aggregate.html#array_agg
            "ListaggOverflowClauseSegment".into(),
            Sequence::new(vec_of_erased![
                Ref::keyword("ON"),
                Ref::keyword("OVERFLOW"),
                one_of(vec_of_erased![
                    Ref::keyword("ERROR"),
                    Sequence::new(vec_of_erased![
                        Ref::keyword("TRUNCATE"),
                        Ref::new("SingleQuotedIdentifierSegment").optional(),
                        one_of(vec_of_erased![
                            Ref::keyword("WITH"),
                            Ref::keyword("WITHOUT")
                        ])
                        .config(|config| {
                            config.optional();
                        }),
                        Ref::keyword("COUNT").optional()
                    ]),
                ]),
            ])
            .to_matchable()
            .into(),
        ),
        (
            // An WITHIN GROUP clause for window functions.
            // https://trino.io/docs/current/functions/aggregate.html#array_agg
            // Trino supports an optional FILTER during aggregation that comes
            // immediately after the WITHIN GROUP clause.
            // https://trino.io/docs/current/functions/aggregate.html#filtering-during-aggregation
            "WithinGroupClauseSegment".into(),
            Sequence::new(vec_of_erased![
                Ref::keyword("WITHIN"),
                Ref::keyword("GROUP"),
                Bracketed::new(vec_of_erased![Ref::new("OrderByClauseSegment")]),
                Ref::new("FilterClauseGrammar").optional(),
            ])
            .to_matchable()
            .into(),
        ),
        (
            // `COMMENT ON` statement.
            // https://trino.io/docs/current/sql/comment.html
            "CommentOnStatementSegment".into(),
            Sequence::new(vec_of_erased![
                Ref::keyword("COMMENT"),
                Ref::keyword("ON"),
                Sequence::new(vec_of_erased![
                    one_of(vec_of_erased![
                        Sequence::new(vec_of_erased![
                            one_of(vec_of_erased![
                                Ref::keyword("TABLE"),
                                // TODO: Create a ViewReferenceSegment
                                Ref::keyword("VIEW"),
                            ]),
                            Ref::new("TableReferenceSegment"),
                        ]),
                        Sequence::new(vec_of_erased![
                            Ref::keyword("COLUMN"),
                            // TODO: Does this correctly emit a Table Reference?
                            Ref::new("ColumnReferenceSegment"),
                        ]),
                    ]),
                    Sequence::new(vec_of_erased![
                        Ref::keyword("IS"),
                        one_of(vec_of_erased![
                            Ref::new("QuotedLiteralSegment"),
                            Ref::keyword("NULL")
                        ]),
                    ]),
                ]),
            ])
            .to_matchable()
            .into(),
        ),
        (
            "IntervalExpressionSegment".into(),
            NodeMatcher::new(
                SyntaxKind::IntervalExpression,
                Sequence::new(vec_of_erased![
                    Ref::keyword("INTERVAL"),
                    Ref::new("QuotedLiteralSegment"),
                    one_of(vec_of_erased![
                        Ref::keyword("YEAR"),
                        Ref::keyword("MONTH"),
                        Ref::keyword("DAY"),
                        Ref::keyword("HOUR"),
                        Ref::keyword("MINUTE"),
                        Ref::keyword("SECOND")
                    ])
                ])
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
        (
            "FrameClauseSegment".into(),
            NodeMatcher::new(SyntaxKind::FrameClause, {
                let frame_extent = one_of(vec_of_erased![
                    Sequence::new(vec_of_erased![Ref::keyword("CURRENT"), Ref::keyword("ROW"),]),
                    Sequence::new(vec_of_erased![
                        one_of(vec_of_erased![
                            Ref::new("NumericLiteralSegment"),
                            Ref::new("DateTimeLiteralGrammar"),
                            Ref::keyword("UNBOUNDED"),
                        ]),
                        one_of(vec_of_erased![
                            Ref::keyword("PRECEDING"),
                            Ref::keyword("FOLLOWING"),
                        ]),
                    ]),
                ]);

                Sequence::new(vec_of_erased![
                    Ref::new("FrameClauseUnitGrammar"),
                    one_of(vec_of_erased![
                        frame_extent.clone(),
                        Sequence::new(vec_of_erased![
                            Ref::keyword("BETWEEN"),
                            frame_extent.clone(),
                            Ref::keyword("AND"),
                            frame_extent
                        ])
                    ])
                ])
                .to_matchable()
            })
            .to_matchable()
            .into(),
        ),
        (
            "SetOperatorSegment".into(),
            NodeMatcher::new(
                SyntaxKind::SetOperator,
                one_of(vec_of_erased![
                    Sequence::new(vec_of_erased![
                        Ref::keyword("UNION"),
                        one_of(vec_of_erased![
                            Ref::keyword("DISTINCT"),
                            Ref::keyword("ALL")
                        ])
                        .config(|config| {
                            config.optional();
                        })
                    ]),
                    Sequence::new(vec_of_erased![
                        one_of(vec_of_erased![
                            Ref::keyword("INTERSECT"),
                            Ref::keyword("EXCEPT")
                        ]),
                        Ref::keyword("ALL").optional()
                    ])
                ])
                .config(|config| {
                    config.exclude = Sequence::new(vec_of_erased![
                        Ref::keyword("EXCEPT"),
                        Bracketed::new(vec_of_erased![Anything::new()])
                    ])
                    .to_matchable()
                    .into();
                })
                .to_matchable(),
            )
            .to_matchable()
            .into(),
        ),
    ]);

    trino_dialect.config(|dialect| dialect.expand())
}
