//! Layering: defaults < file < environment < command line.
//!
//! Each layer is reduced to `(setting, value, source)` triples. Every triple is checked on its
//! own by deserializing a one-key table into [`Config`], so a bad value is reported with its key
//! and its source (the combined table cannot say which layer was wrong). The winners are then
//! merged, deserialized once more, and validated.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;

use toml::{Table, Value};

use super::error::{ConfigError, Source};
use super::keys::{self, KeyDef, Kind};
use super::validate::validate;
use super::{Config, ENV_PREFIX};

/// The text of a config file already read from disk.
#[derive(Debug, Clone, Copy)]
pub struct FileInput<'a> {
    /// Where it was read from (used in messages).
    pub path: &'a Path,
    /// Its contents.
    pub text: &'a str,
    /// The file is readable by group or others.
    pub loose_permissions: bool,
}

/// A value given on the command line, as text.
#[derive(Debug, Clone)]
pub struct CliValue {
    pub(crate) def: &'static KeyDef,
    pub(crate) text: String,
}

/// Where each setting's winning value came from. Settings not listed use the default.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Sources(BTreeMap<&'static str, Source>);

impl Sources {
    /// The source of `key`.
    #[must_use]
    pub fn get(&self, key: &str) -> Source {
        self.0.get(key).cloned().unwrap_or(Source::Default)
    }

    /// Whether `key` was set by something other than the default.
    #[must_use]
    pub fn is_set(&self, key: &str) -> bool {
        self.0.contains_key(key)
    }
}

/// A validated configuration and where it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assembled {
    /// The settings.
    pub config: Config,
    /// Where each came from.
    pub sources: Sources,
    /// Things worth telling the sysop that are not errors.
    pub warnings: Vec<String>,
}

/// Merge the layers, check them, and validate the result. Pure: no I/O.
pub fn assemble(
    file: Option<FileInput<'_>>,
    env: &[(String, OsString)],
    cli: &[CliValue],
) -> Result<Assembled, ConfigError> {
    let mut chosen: BTreeMap<&'static str, (Value, Source)> = BTreeMap::new();
    let mut warnings = Vec::new();

    if let Some(file) = file {
        let origin = Source::File(file.path.to_path_buf());
        for (def, value) in read_file(file)? {
            check_leaf(def, &value, &origin)?;
            if def.secret && file.loose_permissions {
                warnings.push(format!(
                    "{} sets `{}`, which can hold a password, and is readable by group or \
                     others; run `chmod 600` on it",
                    file.path.display(),
                    def.path
                ));
            }
            chosen.insert(def.path, (value, origin.clone()));
        }
    }

    let mut env: Vec<&(String, OsString)> = env
        .iter()
        .filter(|(name, _)| name.starts_with(ENV_PREFIX) && name != super::CONFIG_ENV)
        .collect();
    env.sort_by(|a, b| a.0.cmp(&b.0));
    for (name, raw) in env {
        let def =
            keys::by_env(name).ok_or_else(|| ConfigError::UnknownEnv { name: name.clone() })?;
        let origin = Source::Env(def.env);
        let text = raw
            .to_str()
            .ok_or_else(|| ConfigError::invalid(def.path, &origin, "is not valid UTF-8"))?;
        let value =
            parse_scalar(def, text).map_err(|m| ConfigError::invalid(def.path, &origin, m))?;
        check_leaf(def, &value, &origin)?;
        chosen.insert(def.path, (value, origin));
    }

    for item in cli {
        let flag = item.def.flag.unwrap_or("command line");
        let origin = Source::Flag(flag);
        let value = parse_scalar(item.def, &item.text)
            .map_err(|m| ConfigError::invalid(item.def.path, &origin, m))?;
        check_leaf(item.def, &value, &origin)?;
        chosen.insert(item.def.path, (value, origin));
    }

    let mut merged = Table::new();
    let mut sources = BTreeMap::new();
    for (path, (value, origin)) in chosen {
        insert_path(&mut merged, path, value);
        sources.insert(path, origin);
    }
    let config: Config = Value::Table(merged)
        .try_into()
        .map_err(|e: toml::de::Error| ConfigError::Invalid {
            key: "(combined settings)".to_owned(),
            origin: Source::Default,
            message: e.message().to_owned(),
        })?;
    let sources = Sources(sources);
    validate(&config, &sources)?;
    warnings.extend(super::unused_warnings(&config));
    Ok(Assembled {
        config,
        sources,
        warnings,
    })
}

/// Parse a file into `(setting, value)` pairs, rejecting unknown keys by name.
fn read_file(file: FileInput<'_>) -> Result<Vec<(&'static KeyDef, Value)>, ConfigError> {
    let table: Table = file.text.parse().map_err(|e: toml::de::Error| {
        // `message()` and the line only: the full error would quote the offending source line,
        // which may hold a password.
        let line = e.span().map(|s| {
            file.text[..s.start.min(file.text.len())]
                .matches('\n')
                .count()
                + 1
        });
        ConfigError::Syntax {
            path: file.path.to_path_buf(),
            line,
            message: e.message().to_owned(),
        }
    })?;
    let origin = Source::File(file.path.to_path_buf());
    let mut out = Vec::new();
    for (name, value) in table {
        if let Some(def) = keys::by_path(&name) {
            out.push((def, value));
            continue;
        }
        let is_section = keys::KEYS
            .iter()
            .any(|k| keys::split(k.path).0 == Some(name.as_str()));
        match (is_section, value) {
            (true, Value::Table(section)) => {
                for (leaf, value) in section {
                    let path = format!("{name}.{leaf}");
                    let def = keys::by_path(&path).ok_or_else(|| ConfigError::UnknownKey {
                        key: path.clone(),
                        origin: origin.clone(),
                    })?;
                    out.push((def, value));
                }
            }
            (true, _) => {
                return Err(ConfigError::invalid(
                    &name,
                    &origin,
                    format!("`{name}` is a section; write it as [{name}] with keys under it"),
                ));
            }
            (false, _) => return Err(ConfigError::UnknownKey { key: name, origin }),
        }
    }
    Ok(out)
}

/// Turn environment or flag text into the TOML value its setting expects. Never echoes the text.
fn parse_scalar(def: &KeyDef, text: &str) -> Result<Value, String> {
    match def.kind {
        Kind::Str => Ok(Value::String(text.to_owned())),
        Kind::Int => text
            .trim()
            .parse::<i64>()
            .map(Value::Integer)
            .map_err(|_| "must be a whole number".to_owned()),
        Kind::Bool => match text.trim() {
            "true" => Ok(Value::Boolean(true)),
            "false" => Ok(Value::Boolean(false)),
            _ => Err("must be `true` or `false`".to_owned()),
        },
    }
}

/// Check one value against the typed struct by itself.
fn check_leaf(def: &KeyDef, value: &Value, origin: &Source) -> Result<(), ConfigError> {
    let mut table = Table::new();
    insert_path(&mut table, def.path, value.clone());
    Value::Table(table)
        .try_into::<Config>()
        .map(drop)
        .map_err(|e| {
            let message = if def.secret {
                "must be a string".to_owned()
            } else {
                e.message().to_owned()
            };
            ConfigError::invalid(def.path, origin, message)
        })
}

/// Set `a.b` in `table` to `value`, creating the section.
fn insert_path(table: &mut Table, path: &str, value: Value) {
    match keys::split(path) {
        (None, leaf) => {
            table.insert(leaf.to_owned(), value);
        }
        (Some(section), leaf) => {
            let entry = table
                .entry(section.to_owned())
                .or_insert_with(|| Value::Table(Table::new()));
            if let Value::Table(inner) = entry {
                inner.insert(leaf.to_owned(), value);
            }
        }
    }
}
