use itertools::Itertools;

use crate::{
    core::parser::{
        lexer::Matcher, matchable::Matchable, parsers::StringParser,
        segments::keyword::KeywordSegment, types::DialectElementType,
    },
    helpers::capitalize,
};
use std::{
    borrow::Cow,
    collections::{hash_map::Entry, HashMap, HashSet},
    fmt::Debug,
};

#[derive(Debug, Clone, Default)]
pub struct Dialect {
    root_segment_name: &'static str,
    lexer_matchers: Option<Vec<Box<dyn Matcher>>>,
    // TODO: Can we use PHF here? https://crates.io/crates/phf
    library: HashMap<Cow<'static, str>, DialectElementType>,
    sets: HashMap<&'static str, HashSet<&'static str>>,
    bracket_collections: HashMap<String, HashSet<BracketPair>>,
}

impl Dialect {
    pub fn new(root_segment_name: &'static str) -> Self {
        let mut this = Dialect::default();
        this.root_segment_name = root_segment_name;
        this
    }

    pub fn extend(
        &mut self,
        iter: impl IntoIterator<Item = (Cow<'static, str>, DialectElementType)> + Clone,
    ) {
        check_unique_names(self, &iter.clone().into_iter().collect_vec());

        self.library.extend(iter);
    }

    pub fn lexer_matchers(&self) -> &[Box<dyn Matcher>] {
        match &self.lexer_matchers {
            Some(lexer_matchers) => lexer_matchers,
            None => panic!("Lexing struct has not been set for dialect {self:?}"),
        }
    }

    pub fn set_lexer_matchers(&mut self, lexer_matchers: Vec<Box<dyn Matcher>>) {
        self.lexer_matchers = lexer_matchers.into();
    }

    pub fn sets(&self, label: &str) -> HashSet<&'static str> {
        match label {
            "bracket_pairs" | "angle_bracket_pairs" => {
                panic!("Use `bracket_sets` to retrieve {} set.", label);
            }
            _ => (),
        }

        self.sets.get(label).cloned().unwrap_or_default()
    }

    pub fn sets_mut(&mut self, label: &'static str) -> &mut HashSet<&'static str> {
        assert!(
            label != "bracket_pairs" && label != "angle_bracket_pairs",
            "Use `bracket_sets` to retrieve {} set.",
            label
        );

        match self.sets.entry(label) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => entry.insert(HashSet::new()),
        }
    }

    pub fn update_keywords_set_from_multiline_string(
        &mut self,
        set_label: &'static str,
        values: &'static str,
    ) {
        let keywords = values.lines().map(str::trim);
        self.sets_mut(set_label).extend(keywords);
    }

    pub fn bracket_sets(&self, label: &str) -> HashSet<BracketPair> {
        assert!(
            label == "bracket_pairs" || label == "angle_bracket_pairs",
            "Invalid bracket set. Consider using another identifier instead."
        );

        self.bracket_collections
            .get(label)
            .cloned()
            .unwrap_or_default()
    }

    pub fn bracket_sets_mut(&mut self, label: &str) -> &mut HashSet<BracketPair> {
        assert!(
            label == "bracket_pairs" || label == "angle_bracket_pairs",
            "Invalid bracket set. Consider using another identifier instead."
        );

        self.bracket_collections
            .entry(label.to_string())
            .or_default()
    }

    pub fn update_bracket_sets(&mut self, label: &str, pairs: Vec<BracketPair>) {
        let set = self.bracket_sets_mut(label);
        for pair in pairs {
            set.insert(pair);
        }
    }

    pub fn r#ref(&self, name: &str) -> Box<dyn Matchable> {
        // TODO:
        // if !self.expanded {
        //     panic!("Dialect must be expanded before use.");
        // }

        match self.library.get(name) {
            Some(DialectElementType::Matchable(matchable)) => matchable.clone(),
            Some(DialectElementType::SegmentGenerator(_)) => {
                panic!("Unexpected SegmentGenerator while fetching '{}'", name);
            }
            None => {
                if let Some(keyword) = name.strip_suffix("KeywordSegment") {
                    let keyword_tip = "\
                        \n\nThe syntax in the query is not (yet?) supported. Try to \
                        narrow down your query to a minimal, reproducible case and \
                        raise an issue on GitHub.\n\n\
                        Or, even better, see this guide on how to help contribute \
                        keyword and/or dialect updates:\n\
                        https://github.com/quarylabs/sqruff";
                    panic!(
                        "Grammar refers to the '{keyword}' keyword which was not found in the dialect.{keyword_tip}",
                    );
                } else {
                    panic!("Grammar refers to '{name}' which was not found in the dialect.",);
                }
            }
        }
    }

    pub fn expand(&mut self) {
        // Temporarily take ownership of 'library' from 'self' to avoid borrow checker errors during mutation.
        let mut library = std::mem::take(&mut self.library);
        for element in library.values_mut() {
            if let DialectElementType::SegmentGenerator(generator) = element {
                *element = DialectElementType::Matchable(generator.expand(self));
            }
        }
        self.library = library;

        for keyword_set in ["unreserved_keywords", "reserved_keywords"] {
            if let Some(keywords) = self.sets.get(keyword_set) {
                for kw in keywords {
                    let n = format!("{}KeywordSegment", capitalize(kw));
                    if !self.library.contains_key(n.as_str()) {
                        let parser = StringParser::new(
                            &kw.to_lowercase(),
                            |segment| {
                                Box::new(KeywordSegment::new(
                                    segment.get_raw().unwrap().clone(),
                                    segment.get_position_marker().unwrap(),
                                ))
                            },
                            None,
                            false,
                            None,
                        );

                        self.library
                            .insert(n.into(), DialectElementType::Matchable(Box::new(parser)));
                    }
                }
            }
        }
    }

    pub fn root_segment_name(&self) -> &'static str {
        self.root_segment_name
    }

    pub fn get_root_segment(&self) -> Box<dyn Matchable> {
        self.r#ref(self.root_segment_name())
    }
}

fn check_unique_names(dialect: &Dialect, xs: &[(Cow<'static, str>, DialectElementType)]) {
    let mut names = HashSet::new();

    for (name, _) in xs {
        assert!(
            names.insert(name),
            "ERROR: the name {name} is already registered."
        );

        assert!(
            !dialect.library.contains_key(name),
            "ERROR: the name '{}' is repeated.",
            name
        );
    }
}

pub type BracketPair = (String, String, String, bool);
