use std::ops::Index;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use ahash::AHashMap;
use configparser::ini::Ini;
use itertools::Itertools;
use sqruff_lib_core::dialects::base::Dialect;
use sqruff_lib_core::dialects::init::{dialect_readout, DialectKind};
use sqruff_lib_core::errors::SQLFluffUserError;
use sqruff_lib_core::parser::parser::Parser;
use sqruff_lib_dialects::kind_to_dialect;

use crate::utils::reflow::config::ReflowConfig;

/// split_comma_separated_string takes a string and splits it on commas and
/// trims and filters out empty strings.
pub fn split_comma_separated_string(raw_str: &str) -> Value {
    let values = raw_str
        .split(',')
        .filter_map(|x| {
            let trimmed = x.trim();
            (!trimmed.is_empty()).then(|| Value::String(trimmed.into()))
        })
        .collect();
    Value::Array(values)
}

/// The class that actually gets passed around as a config object.
// TODO This is not a translation that is particularly accurate.
#[derive(Debug, PartialEq, Clone)]
pub struct FluffConfig {
    pub(crate) indentation: FluffConfigIndentation,
    pub raw: AHashMap<String, Value>,
    extra_config_path: Option<String>,
    _configs: AHashMap<String, AHashMap<String, String>>,
    pub(crate) dialect: Dialect,
    sql_file_exts: Vec<String>,
    reflow: ReflowConfig,
}

impl Default for FluffConfig {
    fn default() -> Self {
        Self::new(<_>::default(), None, None)
    }
}

impl FluffConfig {
    pub fn get(&self, key: &str, section: &str) -> &Value {
        &self.raw[section][key]
    }

    pub fn reflow(&self) -> &ReflowConfig {
        &self.reflow
    }

    pub fn reload_reflow(&mut self) {
        self.reflow = ReflowConfig::from_fluff_config(self);
    }

    /// from_source creates a config object from a string. This is used for testing and for
    /// loading a config from a string.
    ///
    /// The optional_path_specification is used to specify a path to use for relative paths in the
    /// config. This is useful for testing.
    pub fn from_source(source: &str, optional_path_specification: Option<&Path>) -> FluffConfig {
        let configs = ConfigLoader::from_source(source, optional_path_specification);
        FluffConfig::new(configs, None, None)
    }

    pub fn get_section(&self, section: &str) -> &AHashMap<String, Value> {
        self.raw[section].as_map().unwrap()
    }

    // TODO This is not a translation that is particularly accurate.
    pub fn new(
        configs: AHashMap<String, Value>,
        extra_config_path: Option<String>,
        indentation: Option<FluffConfigIndentation>,
    ) -> Self {
        fn nested_combine(
            mut a: AHashMap<String, Value>,
            b: AHashMap<String, Value>,
        ) -> AHashMap<String, Value> {
            for (key, value_b) in b {
                match (a.get(&key), value_b) {
                    (Some(Value::Map(map_a)), Value::Map(map_b)) => {
                        let combined = nested_combine(map_a.clone(), map_b);
                        a.insert(key, Value::Map(combined));
                    }
                    (_, value) => {
                        a.insert(key, value);
                    }
                }
            }
            a
        }

        let values = ConfigLoader::get_config_elems_from_file(
            None,
            include_str!("./default_config.cfg").into(),
        );

        let mut defaults = AHashMap::new();
        ConfigLoader::incorporate_vals(&mut defaults, values);

        let mut configs = nested_combine(defaults, configs);

        let dialect = match configs
            .get("core")
            .and_then(|map| map.as_map().unwrap().get("dialect"))
        {
            None => DialectKind::default(),
            Some(Value::String(std)) => DialectKind::from_str(std).unwrap(),
            _value => DialectKind::default(),
        };

        let dialect = kind_to_dialect(&dialect);
        for (in_key, out_key) in [
            // Deal with potential ignore & warning parameters
            ("ignore", "ignore"),
            ("warnings", "warnings"),
            ("rules", "rule_allowlist"),
            // Allowlists and denylistsignore_words
            ("exclude_rules", "rule_denylist"),
        ] {
            match configs["core"].as_map().unwrap().get(in_key) {
                Some(value) if !value.is_none() => {
                    let string = value.as_string().unwrap();
                    let values = split_comma_separated_string(string);

                    configs
                        .get_mut("core")
                        .unwrap()
                        .as_map_mut()
                        .unwrap()
                        .insert(out_key.into(), values);
                }
                _ => {}
            }
        }

        let sql_file_exts = configs["core"]["sql_file_exts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|it| it.as_string().unwrap().to_owned())
            .collect();

        let mut this = Self {
            raw: configs,
            dialect: dialect
                .expect("Dialect is disabled. Please enable the corresponding feature."),
            extra_config_path,
            _configs: AHashMap::new(),
            indentation: indentation.unwrap_or_default(),
            sql_file_exts,
            reflow: ReflowConfig::default(),
        };
        this.reflow = ReflowConfig::from_fluff_config(&this);
        this
    }

    pub fn with_sql_file_exts(mut self, exts: Vec<String>) -> Self {
        self.sql_file_exts = exts;
        self
    }

    /// Loads a config object just based on the root directory.
    // TODO This is not a translation that is particularly accurate.
    pub fn from_root(
        extra_config_path: Option<String>,
        ignore_local_config: bool,
        overrides: Option<AHashMap<String, String>>,
    ) -> Result<FluffConfig, SQLFluffUserError> {
        let loader = ConfigLoader {};
        let mut config =
            loader.load_config_up_to_path(".", extra_config_path.clone(), ignore_local_config);

        if let Some(overrides) = overrides {
            if let Some(dialect) = overrides.get("dialect") {
                let core = config
                    .entry("core".into())
                    .or_insert_with(|| Value::Map(AHashMap::new()));

                core.as_map_mut()
                    .unwrap()
                    .insert("dialect".into(), Value::String(dialect.clone().into()));
            }
        }

        Ok(FluffConfig::new(config, extra_config_path, None))
    }

    pub fn from_kwargs(
        config: Option<FluffConfig>,
        dialect: Option<Dialect>,
        rules: Option<Vec<String>>,
    ) -> Self {
        if (dialect.is_some() || rules.is_some()) && config.is_some() {
            panic!(
                "Cannot specify `config` with `dialect` or `rules`. Any config object specifies \
                 its own dialect and rules."
            )
        } else {
            config.unwrap()
        }
    }

    /// Process a full raw file for inline config and update self.
    pub fn process_raw_file_for_config(&self, raw_str: &str) {
        // Scan the raw file for config commands
        for raw_line in raw_str.lines() {
            if raw_line.to_string().starts_with("-- sqlfluff") {
                // Found an in-file config command
                self.process_inline_config(raw_line)
            }
        }
    }

    /// Process an inline config command and update self.
    pub fn process_inline_config(&self, _config_line: &str) {
        panic!("Not implemented")
    }

    /// Check if the config specifies a dialect, raising an error if not.
    pub fn verify_dialect_specified(&self) -> Option<SQLFluffUserError> {
        if self._configs.get("core")?.get("dialect").is_some() {
            return None;
        }
        // Get list of available dialects for the error message. We must
        // import here rather than at file scope in order to avoid a circular
        // import.
        Some(SQLFluffUserError::new(format!(
            "No dialect was specified. You must configure a dialect or
specify one on the command line using --dialect after the
command. Available dialects: {}",
            dialect_readout().join(", ").as_str()
        )))
    }

    pub fn get_dialect(&self) -> &Dialect {
        &self.dialect
    }

    pub fn sql_file_exts(&self) -> &[String] {
        self.sql_file_exts.as_ref()
    }
}

#[derive(Debug, PartialEq, Clone)]
pub struct FluffConfigIndentation {
    pub template_blocks_indent: bool,
}

impl Default for FluffConfigIndentation {
    fn default() -> Self {
        Self {
            template_blocks_indent: true,
        }
    }
}

pub struct ConfigLoader;

impl ConfigLoader {
    #[allow(unused_variables)]
    fn iter_config_locations_up_to_path(
        path: &Path,
        working_path: Option<&Path>,
        ignore_local_config: bool,
    ) -> impl Iterator<Item = PathBuf> {
        let mut given_path = std::path::absolute(path).unwrap();
        let working_path = std::env::current_dir().unwrap();

        if !given_path.is_dir() {
            given_path = given_path.parent().unwrap().into();
        }

        let common_path = common_path::common_path(&given_path, working_path).unwrap();
        let mut path_to_visit = common_path;

        let head = Some(given_path.canonicalize().unwrap()).into_iter();
        let tail = std::iter::from_fn(move || {
            if path_to_visit != given_path {
                let path = path_to_visit.canonicalize().unwrap();

                let next_path_to_visit = {
                    // Convert `path_to_visit` & `given_path` to `Path`
                    let path_to_visit_as_path = path_to_visit.as_path();
                    let given_path_as_path = given_path.as_path();

                    // Attempt to create a relative path from `given_path` to `path_to_visit`
                    match given_path_as_path.strip_prefix(path_to_visit_as_path) {
                        Ok(relative_path) => {
                            // Get the first component of the relative path
                            if let Some(first_part) = relative_path.components().next() {
                                // Combine `path_to_visit` with the first part of the relative path
                                path_to_visit.join(first_part.as_os_str())
                            } else {
                                // If there are no components in the relative path, return
                                // `path_to_visit`
                                path_to_visit.clone()
                            }
                        }
                        Err(_) => {
                            // If `given_path` is not relative to `path_to_visit`, handle the error
                            // (e.g., return `path_to_visit`)
                            // This part depends on how you want to handle the error.
                            path_to_visit.clone()
                        }
                    }
                };

                if next_path_to_visit == path_to_visit {
                    return None;
                }

                path_to_visit = next_path_to_visit;

                Some(path)
            } else {
                None
            }
        });

        head.chain(tail)
    }

    pub fn load_config_up_to_path(
        &self,
        path: impl AsRef<Path>,
        extra_config_path: Option<String>,
        ignore_local_config: bool,
    ) -> AHashMap<String, Value> {
        let path = path.as_ref();

        let config_stack = if ignore_local_config {
            extra_config_path
                .map(|path| vec![self.load_config_at_path(path)])
                .unwrap_or_default()
        } else {
            let configs = Self::iter_config_locations_up_to_path(path, None, ignore_local_config);
            configs
                .map(|path| self.load_config_at_path(path))
                .collect_vec()
        };

        nested_combine(config_stack)
    }

    pub fn load_config_at_path(&self, path: impl AsRef<Path>) -> AHashMap<String, Value> {
        let path = path.as_ref();

        let filename_options = [
            /* "setup.cfg", "tox.ini", "pep8.ini", */
            ".sqlfluff",
            ".sqruff", /* "pyproject.toml" */
        ];

        let mut configs = AHashMap::new();

        if path.is_dir() {
            for fname in filename_options {
                let path = path.join(fname);
                if path.exists() {
                    ConfigLoader::load_config_file(path, &mut configs);
                }
            }
        } else if path.is_file() {
            ConfigLoader::load_config_file(path, &mut configs);
        };

        configs
    }

    pub fn from_source(source: &str, path: Option<&Path>) -> AHashMap<String, Value> {
        let mut configs = AHashMap::new();
        let elems = ConfigLoader::get_config_elems_from_file(path, Some(source));
        ConfigLoader::incorporate_vals(&mut configs, elems);
        configs
    }

    pub fn load_config_file(path: impl AsRef<Path>, configs: &mut AHashMap<String, Value>) {
        let elems = ConfigLoader::get_config_elems_from_file(path.as_ref().into(), None);
        ConfigLoader::incorporate_vals(configs, elems);
    }

    fn get_config_elems_from_file(
        path: Option<&Path>,
        config_string: Option<&str>,
    ) -> Vec<(Vec<String>, Value)> {
        let mut buff = Vec::new();
        let mut config = Ini::new();

        let content = match (path, config_string) {
            (None, None) | (Some(_), Some(_)) => {
                unimplemented!("One of fpath or config_string is required.")
            }
            (None, Some(text)) => text.to_owned(),
            (Some(path), None) => std::fs::read_to_string(path).unwrap(),
        };

        config.read(content).unwrap();

        for section in config.sections() {
            let key = if section == "sqlfluff" || section == "sqruff" {
                vec!["core".to_owned()]
            } else if let Some(key) = section
                .strip_prefix("sqlfluff:")
                .or_else(|| section.strip_prefix("sqruff:"))
            {
                key.split(':').map(ToOwned::to_owned).collect()
            } else {
                continue;
            };

            let config_map = config.get_map_ref();
            if let Some(section) = config_map.get(&section) {
                for (name, value) in section {
                    let value: Value = value.as_ref().unwrap().parse().unwrap();
                    let name_lowercase = name.to_lowercase();

                    if name_lowercase == "load_macros_from_path" {
                        unimplemented!()
                    } else if name_lowercase.ends_with("_path") || name_lowercase.ends_with("_dir")
                    {
                        // if absolute_path, just keep
                        // if relative path, make it absolute
                        let path = PathBuf::from(value.as_string().unwrap());
                        if !path.is_absolute() {
                            unimplemented!(
                                "Relative paths are not supported yet. {}, {}",
                                name,
                                path.to_str().unwrap_or_default()
                            );
                        }
                    }

                    let mut key = key.clone();
                    key.push(name.clone());
                    buff.push((key, value));
                }
            }
        }

        buff
    }

    fn incorporate_vals(ctx: &mut AHashMap<String, Value>, values: Vec<(Vec<String>, Value)>) {
        for (path, value) in values {
            let mut current_map = &mut *ctx;
            for key in path.iter().take(path.len() - 1) {
                match current_map
                    .entry(key.to_string())
                    .or_insert_with(|| Value::Map(AHashMap::new()))
                    .as_map_mut()
                {
                    Some(slot) => current_map = slot,
                    None => panic!("Overriding config value with section! [{path:?}]"),
                }
            }

            let last_key = path.last().expect("Expected at least one element in path");
            current_map.insert(last_key.to_string(), value);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, serde::Deserialize)]
#[serde(untagged)]
pub enum Value {
    Int(i32),
    Bool(bool),
    Float(f64),
    String(Box<str>),
    Map(AHashMap<String, Value>),
    Array(Vec<Value>),
    #[default]
    None,
}

impl Value {
    pub fn is_none(&self) -> bool {
        matches!(self, Value::None)
    }

    pub fn as_array(&self) -> Option<Vec<Value>> {
        match self {
            Self::Array(v) => Some(v.clone()),
            Self::String(q) => {
                let xs = q
                    .split(',')
                    .map(|it| Value::String(it.into()))
                    .collect_vec();
                Some(xs)
            }
            Self::Bool(b) => Some(vec![Value::String(b.to_string().into())]),
            _ => None,
        }
    }
}

impl Index<&str> for Value {
    type Output = Value;

    fn index(&self, index: &str) -> &Self::Output {
        match self {
            Value::Map(map) => map.get(index).unwrap_or(&Value::None),
            _ => unreachable!(),
        }
    }
}

impl Value {
    pub fn to_bool(&self) -> bool {
        match *self {
            Value::Int(v) => v != 0,
            Value::Bool(v) => v,
            Value::Float(v) => v != 0.0,
            Value::String(ref v) => !v.is_empty(),
            Value::Map(ref v) => !v.is_empty(),
            Value::None => false,
            Value::Array(ref v) => !v.is_empty(),
        }
    }

    pub fn map<T>(&self, f: impl Fn(&Self) -> T) -> Option<T> {
        if self == &Value::None {
            return None;
        }

        Some(f(self))
    }
    pub fn as_map(&self) -> Option<&AHashMap<String, Value>> {
        if let Self::Map(map) = self {
            Some(map)
        } else {
            None
        }
    }

    pub fn as_map_mut(&mut self) -> Option<&mut AHashMap<String, Value>> {
        if let Self::Map(map) = self {
            Some(map)
        } else {
            None
        }
    }

    pub fn as_int(&self) -> Option<i32> {
        if let Self::Int(v) = self {
            Some(*v)
        } else {
            None
        }
    }

    pub fn as_string(&self) -> Option<&str> {
        if let Self::String(v) = self {
            Some(v)
        } else {
            None
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        if let Self::Bool(v) = self {
            Some(*v)
        } else {
            None
        }
    }
}

impl FromStr for Value {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        use unicase::UniCase;

        static KEYWORDS: phf::Map<UniCase<&'static str>, Value> = phf::phf_map! {
            UniCase::ascii("true") => Value::Bool(true),
            UniCase::ascii("false") => Value::Bool(false),
            UniCase::ascii("none") => Value::None,
        };

        if let Ok(value) = s.parse() {
            return Ok(Value::Int(value));
        }

        if let Ok(value) = s.parse() {
            return Ok(Value::Float(value));
        }

        let key = UniCase::ascii(s);
        let value = KEYWORDS
            .get(&key)
            .cloned()
            .unwrap_or_else(|| Value::String(Box::from(s)));

        Ok(value)
    }
}

fn nested_combine(config_stack: Vec<AHashMap<String, Value>>) -> AHashMap<String, Value> {
    let capacity = config_stack.len();
    let mut result = AHashMap::with_capacity(capacity);

    for dict in config_stack {
        for (key, value) in dict {
            result.insert(key, value);
        }
    }

    result
}

impl<'a> From<&'a FluffConfig> for Parser<'a> {
    fn from(config: &'a FluffConfig) -> Self {
        let dialect = config.get_dialect();
        let indentation_config = config.raw["indentation"].as_map().unwrap();
        let indentation_config: AHashMap<_, _> = indentation_config
            .iter()
            .map(|(key, value)| (key.clone(), value.to_bool()))
            .collect();
        Self::new(dialect, indentation_config)
    }
}
