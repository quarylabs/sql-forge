use sqruff_lib_core::{
    dialects::{base::Dialect, init::DialectKind, syntax::SyntaxKind},
    helpers::{Config, ToMatchable},
    parser::{
        grammar::{anyof::one_of, base::Ref, sequence::Sequence},
        lexer::Matcher,
    },
    vec_of_erased,
};

use crate::databricks_keywords::{RESERVED_KEYWORDS, UNRESERVED_KEYWORDS};

pub fn dialect() -> Dialect {
    let raw_sparksql = crate::sparksql::dialect();

    let mut databricks = crate::sparksql::dialect();
    databricks.name = DialectKind::Databricks;

    // databricks
    //     .sets_mut("unreserverd_keywords")
    //     .extend(UNRESERVED_KEYWORDS);
    // databricks
    //     .sets_mut("unreserverd_keywords")
    //     .extend(raw_sparksql.sets("reserverd_keywords"));
    // databricks.sets_ut("unreserverd_keywords")

    // databricks.sets_mut("reserverd_keywords").clear();
    // databricks.sets_mut("reserverd_keywords").extend(RESERVED_KEYWORDS);

    // databricks.sets_mut("data_part_function_name").extend(["TIMEDIFF"]);

    // Named Function Parameters:
    // https://docs.databricks.com/en/sql/language-manual/sql-ref-function-invocation.html#named-parameter-invocation
    databricks.insert_lexer_matchers(
        vec![Matcher::string("right_array", "=>", SyntaxKind::RightArrow)],
        "equals",
    );

    // Notebook Cell Delimiter:
    // https://learn.microsoft.com/en-us/azure/databricks/notebooks/notebook-export-import#sql-1
    // // databricks.insert_lexer_matchers(
    //     vec![Match::regex(
    //         "command",
    //         r"(\r?\n){2}-- COMMAND ----------(\r?\n)",
    //         SyntaxKind::Code,
    //     )],
    //     "newline",
    // );

    // Datbricks Notebook Start:
    // Needed to insert "so early" to avoid magic + notebook
    // start to be interpreted as inline comment
    databrikcs.insert_lexer_matchers(
        vec![
            Matcher::regex(
                "notebook_start",
                r"-- Databricks notebook source(\r?\n){1}",
                SyntaxKind::CommentStatement,
            ),
            Matcher::regex(
                "magic_line",
                r"(-- MAGIC)( [^%]{1})([^\n]*)",
                SyntaxKind::Code,
            ),
            Matcher::regex(
                "magic_start",
                r"(-- MAGIC %)([^\n]{2,})(\r?\n)",
                SyntaxKind::CodeSegment,
            ),
        ],
        "inline_comment",
    );

    databricks.add([
        (
            "SetTagsGrammar".into(),
            Sequence::new(vec_of_erased![
                Ref::keyword("SET"),
                Ref::keyword("TAGS"),
                Ref::new("BracketedPropertyListGrammar"),
            ])
            .to_matchable()
            .into(),
        ),
        (
            "UnsetTagsGrammar".into(),
            Sequence::new(vec_of_erased![
                Ref::keyword("UNSET"),
                Ref::keyword("TAGS"),
                Ref::new("BracketedPropertyNameListGrammar"),
            ])
            .to_matchable()
            .into(),
        ),
        (
            "ColumnDefaultGrammar".into(),
            one_of(vec_of_erased!(
                Ref::new("LiteralGrammar"),
                Ref::new("FucntionSegmenet"),
            ))
            .to_matchable()
            .into(),
        ),
        (
            "ConstraintOptionGrammar".into(),
            Sequence::new(vec_of_erased![
                Sequence::new(vec_of_erased![
                    Ref::keyword("ENABLE"),
                    Ref::keyword("NOVALIDATE")
                ])
                .config(|config| { config.optional() }),
                Sequence::new(vec_of_erased![
                    Ref::keyword("NOT"),
                    Ref::keyword("ENFORCED")
                ])
                .config(|config| { config.optional() }),
                Sequence::new(vec_of_erased![Ref::keyword("DEFERRABLE")])
                    .config(|config| { config.optional() }),
                Sequence::new(vec_of_erased![
                    Ref::keyword("INITIALLY"),
                    Ref::keyword("DEFERRED")
                ])
                .config(|config| { config.optional() }),
                one_of(vec_of_erased![Ref::keyword("NORELY"), Ref::keyword("RELY"),])
                    .config(|config| { config.optional() }),
            ])
            .to_matchable()
            .into(),
        ),
        (
            "ForeignKeyOptionGrammar".into(),
            Sequence::new(vec_of_erased![
                Sequence::new(vec_of_erased![Ref::keyword("MATCH"), Ref::keyword("FULL"),])
                    .config(|config| { config.optional() }),
                Sequence::new(vec_of_erased![
                    Ref::keyword("ON"),
                    Ref::keyword("UPDATE"),
                    Ref::keyword("NO"),
                    Ref::keyword("ACTION"),
                ])
                .config(|config| { config.optional() }),
                Sequence::new(vec_of_erased![
                    Ref::keyword("ON"),
                    Ref::keyword("DELETE"),
                    Ref::keyword("NO"),
                    Ref::keyword("ACTION"),
                ]),
            ]),
        ),
        // DropConstraintGrammar=Sequence(
        //     "DROP",
        //     OneOf(
        //         Sequence(
        //             Ref("PrimaryKeyGrammar"),
        //             Ref("IfExistsGrammar", optional=True),
        //             OneOf(
        //                 "RESTRICT",
        //                 "CASCADE",
        //                 optional=True,
        //             ),
        //         ),
        //         Sequence(
        //             Ref("ForeignKeyGrammar"),
        //             Ref("IfExistsGrammar", optional=True),
        //             Bracketed(
        //                 Delimited(
        //                     Ref("ColumnReferenceSegment"),
        //                 )
        //             ),
        //         ),
        //         Sequence(
        //             "CONSTRAINT",
        //             Ref("IfExistsGrammar", optional=True),
        //             Ref("ObjectReferenceSegment"),
        //             OneOf(
        //                 "RESTRICT",
        //                 "CASCADE",
        //                 optional=True,
        //             ),
        //         ),
        //     ),
        // ),
        (
            "DropConstraintGrammar".into(),
            one_of(vec_of_erased![
                Sequence::new(vec_of_erased![
                    Ref::new("PrimaryKeyGrammar"),
                    Ref::new("IfExistsGrammar").optional(),
                    one_of(vec_of_erased![
                        Ref::keyword("RESTRICT"),
                        Ref::keyword("CASCADE"),
                    ])
                    .config(|config| config.optional()),
                ]),
                Sequence::new(vec_of_erased![
                    Ref::new("ForeignKeyGrammar"),
                    Ref::new("IfExistsGrammar").optional(),
                    Ref::new("Bracketed").config(|config| {
                        config.set_children(vec_of_erased![Ref::new("ColumnReferenceSegment")])
                    }),
                ]),
                Sequence::new(vec_of_erased![
                    Ref::keyword("CONSTRAINT"),
                    Ref::new("IfExistsGrammar").optional(),
                    Ref::new("ObjectReferenceSegment"),
                    one_of(vec_of_erased![
                        Ref::keyword("RESTRICT"),
                        Ref::keyword("CASCADE"),
                    ])
                    .config(|config| config.optional()),
                ]),
            ])
            .to_matchable()
            .into(),
        ),
        // AlterPartitionGrammar=Sequence(
        //     "PARTITION",
        //     Bracketed(
        //         Delimited(
        //             AnyNumberOf(
        //                 OneOf(
        //                     Ref("ColumnReferenceSegment"),
        //                     Ref("SetClauseSegment"),
        //                 ),
        //                 min_times=1,
        //             ),
        //         ),
        //     ),
        // ),
        // RowFilterClauseGrammar=Sequence(
        //     "ROW",
        //     "FILTER",
        //     Ref("ObjectReferenceSegment"),
        //     "ON",
        //     Bracketed(
        //         Delimited(
        //             OneOf(
        //                 Ref("ColumnReferenceSegment"),
        //                 Ref("LiteralGrammar"),
        //             ),
        //             optional=True,
        //         ),
        //     ),
        // ),
        // PropertiesBackTickedIdentifierSegment=RegexParser(
        //     r"`.+`",
        //     IdentifierSegment,
        //     type="properties_naked_identifier",
        // ),
        // LocationWithCredentialGrammar=Sequence(
        //     "LOCATION",
        //     Ref("QuotedLiteralSegment"),
        //     Sequence(
        //         "WITH",
        //         Bracketed(
        //             "CREDENTIAL",
        //             Ref("PrincipalIdentifierSegment"),
        //         ),
        //         optional=True,
        //     ),
        // ),
        // NotebookStart=TypedParser("notebook_start", CommentSegment, type="notebook_start"),
        // MagicLineGrammar=TypedParser("magic_line", CodeSegment, type="magic_line"),
        // MagicStartGrammar=TypedParser("magic_start", CodeSegment, type="magic_start"),
        // VariableNameIdentifierSegment=OneOf(
        //     Ref("NakedIdentifierSegment"),
        //     Ref("BackQuotedIdentifierSegment"),
        // ),
    ]);

    databricks.add([
        // https://docs.databricks.com/en/sql/language-manual/sql-ref-syntax-aux-show-views.html
        // Only difference between this and the SparkSQL version:
        // - `LIKE` keyword is optional
        (
            "ShowViewsGrammar".into(),
            Sequence::new(vec_of_erased![
                Ref::keyword("VIEWS"),
                Sequence::new(vec_of_erased![one_of(vec_of_erased![
                    Ref::keyword("FROM"),
                    Ref::keyword("IN"),
                ])])
                .config(|config| {
                    config.optional();
                }),
                Sequence::new(vec_of_erased![
                    Ref::keyword("LIKE").optional(),
                    Ref::new("QuotedLiteralSegment"),
                ])
                .config(|config| { config.optional() })
            ])
            .to_matchable()
            .into(),
        ),
        // TODO Missing Show Object Grammar
        (
            "NotNullGrammar".into(),
            Sequence::new(vec_of_erased![Ref::keyword("NOT"), Ref::keyword("NULL")])
                .to_matchable()
                .into(),
        ),
        // TODO Function NameIdentifierSegment
    ]);

    return databricks;
}
