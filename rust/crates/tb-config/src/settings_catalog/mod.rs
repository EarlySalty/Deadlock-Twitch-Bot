use crate::{file::FileError, file::Schema, global::BotConfig};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    sync::OnceLock,
};

mod fields;
#[cfg(test)]
mod tests;

pub const CATALOG_VERSION: u32 = 1;
const MAX_CHANGES: usize = 512;
const MAX_TEXT_BYTES: usize = 4096;
const MAX_EXACT_JSON_INTEGER: u64 = (1 << 53) - 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Boolean,
    Integer,
    Number,
    String,
    Choice,
    StringList,
    IntegerList,
}

#[derive(Clone, Debug, Serialize)]
pub struct Group {
    pub id: &'static str,
    pub label: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub struct Field {
    pub path: &'static str,
    pub group: &'static str,
    pub label: &'static str,
    pub help: &'static str,
    pub kind: Kind,
    pub nullable: bool,
    pub restart_required: bool,
    pub min: Option<String>,
    pub max: Option<String>,
    pub choices: &'static [&'static str],
    pub min_items: Option<usize>,
    pub max_items: Option<usize>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CatalogResponse {
    pub version: u32,
    pub revision: String,
    pub groups: &'static [Group],
    pub fields: &'static [Field],
    pub values: BTreeMap<&'static str, Value>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChangeRequest {
    pub revision: String,
    pub changes: Vec<Change>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Change {
    pub path: String,
    pub value: Value,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CatalogError {
    StaleRevision,
    UnknownPath,
    InvalidRequest,
    InvalidValue(&'static str),
    InvalidDocument,
    Rejected(FileError),
}

impl CatalogError {
    pub fn message(&self) -> &'static str {
        match self {
            Self::StaleRevision => "Die Konfiguration wurde inzwischen geändert. Bitte neu laden.",
            Self::UnknownPath => "Dieses Feld ist nicht über das Dashboard änderbar.",
            Self::InvalidRequest => {
                "Die Änderungsliste ist leer, zu lang oder enthält ein Feld doppelt."
            }
            Self::InvalidValue(_) => "Ein Wert passt nicht zu Typ, Auswahl oder erlaubtem Bereich.",
            Self::InvalidDocument => "Die gespeicherte Konfiguration ist nicht lesbar.",
            Self::Rejected(_) => "Die geänderte Konfiguration besteht die Gesamtprüfung nicht.",
        }
    }
}

impl fmt::Display for CatalogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for CatalogError {}

pub fn groups() -> &'static [Group] {
    fields::GROUPS
}

pub fn fields() -> &'static [Field] {
    static FIELDS: OnceLock<Vec<Field>> = OnceLock::new();
    FIELDS.get_or_init(fields::catalog)
}

pub fn field(path: &str) -> Option<&'static Field> {
    fields().iter().find(|field| field.path == path)
}

pub fn read_catalog(text: &str) -> Result<CatalogResponse, CatalogError> {
    let document: toml::Table = text.parse().map_err(|_| CatalogError::InvalidDocument)?;
    let mut values = BTreeMap::new();
    for field in fields() {
        let value = match lookup(&document, field.path) {
            Some(value) => wire_value(field, value)?,
            None => Value::Null,
        };
        values.insert(field.path, value);
    }
    Ok(CatalogResponse {
        version: CATALOG_VERSION,
        revision: crate::editor::revision(text),
        groups: groups(),
        fields: fields(),
        values,
    })
}

pub fn apply_changes(text: &str, request: &ChangeRequest) -> Result<String, CatalogError> {
    if crate::editor::revision(text) != request.revision {
        return Err(CatalogError::StaleRevision);
    }
    let mut document: toml_edit::DocumentMut =
        text.parse().map_err(|_| CatalogError::InvalidDocument)?;
    apply_to_document(&mut document, &request.changes)?;
    let candidate = document.to_string();
    validate_candidate(&candidate)?;
    Ok(candidate)
}

pub(crate) fn apply_to_document(
    document: &mut toml_edit::DocumentMut,
    changes: &[Change],
) -> Result<(), CatalogError> {
    if changes.is_empty() || changes.len() > MAX_CHANGES {
        return Err(CatalogError::InvalidRequest);
    }
    let mut seen = BTreeSet::new();
    for change in changes {
        let field = field(&change.path).ok_or(CatalogError::UnknownPath)?;
        if !seen.insert(field.path) {
            return Err(CatalogError::InvalidRequest);
        }
        let value = toml_value(field, &change.value)?;
        let parts: Vec<&str> = field.path.split('.').collect();
        write(document.as_item_mut(), &parts, value)
            .map_err(|()| CatalogError::InvalidValue(field.path))?;
    }
    Ok(())
}

fn validate_candidate(text: &str) -> Result<(), CatalogError> {
    let config: BotConfig = toml::from_str(text).map_err(|_| CatalogError::InvalidDocument)?;
    config.validate().map_err(CatalogError::Rejected)
}

fn lookup<'a>(table: &'a toml::Table, path: &str) -> Option<&'a toml::Value> {
    let mut parts = path.split('.');
    let mut current = table.get(parts.next()?)?;
    for part in parts {
        current = current.as_table()?.get(part)?;
    }
    Some(current)
}

fn wire_value(field: &Field, value: &toml::Value) -> Result<Value, CatalogError> {
    let invalid = || CatalogError::InvalidValue(field.path);
    Ok(match (field.kind, value) {
        (Kind::Boolean, toml::Value::Boolean(flag)) => Value::Bool(*flag),
        (Kind::Integer, toml::Value::Integer(number)) => Value::String(number.to_string()),
        (Kind::Number, toml::Value::Float(number)) => serde_json::Number::from_f64(*number)
            .map(Value::Number)
            .ok_or_else(invalid)?,
        (Kind::Number, toml::Value::Integer(number)) => Value::from(*number as f64),
        (Kind::String | Kind::Choice, toml::Value::String(text)) => Value::String(text.clone()),
        (Kind::StringList, toml::Value::Array(items)) => Value::Array(
            items
                .iter()
                .map(|item| item.as_str().map(|text| Value::String(text.to_owned())))
                .collect::<Option<_>>()
                .ok_or_else(invalid)?,
        ),
        (Kind::IntegerList, toml::Value::Array(items)) => Value::Array(
            items
                .iter()
                .map(|item| {
                    item.as_integer()
                        .map(|number| Value::String(number.to_string()))
                })
                .collect::<Option<_>>()
                .ok_or_else(invalid)?,
        ),
        _ => return Err(invalid()),
    })
}

fn toml_value(field: &Field, value: &Value) -> Result<Option<toml_edit::Value>, CatalogError> {
    let invalid = CatalogError::InvalidValue(field.path);
    if value.is_null() {
        return if field.nullable {
            Ok(None)
        } else {
            Err(invalid)
        };
    }
    let converted = match field.kind {
        Kind::Boolean => value.as_bool().map(toml_edit::Value::from),
        Kind::Integer => integer(field, value).map(toml_edit::Value::from),
        Kind::Number => number(field, value).map(toml_edit::Value::from),
        Kind::String => text(value).map(toml_edit::Value::from),
        Kind::Choice => value
            .as_str()
            .filter(|choice| field.choices.contains(choice))
            .map(toml_edit::Value::from),
        Kind::StringList => list(field, value, |item| text(item).map(toml_edit::Value::from)),
        Kind::IntegerList => list(field, value, |item| {
            integer(field, item).map(toml_edit::Value::from)
        }),
    };
    converted.map(Some).ok_or(invalid)
}

fn integer(field: &Field, value: &Value) -> Option<i64> {
    let raw = value.as_str()?;
    let parsed = raw.parse::<i64>().ok()?;
    let min = field.min.as_deref()?.parse::<i64>().ok()?;
    let max = field.max.as_deref()?.parse::<i64>().ok()?;
    (parsed.to_string() == raw && (min..=max).contains(&parsed)).then_some(parsed)
}

fn number(field: &Field, value: &Value) -> Option<f64> {
    let raw = value.as_number()?;
    let parsed = if let Some(whole) = raw.as_i64() {
        (whole.unsigned_abs() <= MAX_EXACT_JSON_INTEGER).then_some(whole as f64)?
    } else if let Some(whole) = raw.as_u64() {
        (whole <= MAX_EXACT_JSON_INTEGER).then_some(whole as f64)?
    } else {
        raw.as_f64()?
    };
    let above_min = field.min.as_deref().map_or(Some(true), |min| {
        min.parse::<f64>().ok().map(|min| parsed >= min)
    })?;
    let below_max = field.max.as_deref().map_or(Some(true), |max| {
        max.parse::<f64>().ok().map(|max| parsed <= max)
    })?;
    (parsed.is_finite() && above_min && below_max).then_some(parsed)
}

fn text(value: &Value) -> Option<String> {
    let raw = value.as_str()?;
    (raw.len() <= MAX_TEXT_BYTES && !raw.chars().any(char::is_control)).then(|| raw.to_owned())
}

fn list(
    field: &Field,
    value: &Value,
    item: impl Fn(&Value) -> Option<toml_edit::Value>,
) -> Option<toml_edit::Value> {
    let items = value.as_array()?;
    let fits = field.min_items.is_none_or(|min| items.len() >= min)
        && field.max_items.is_none_or(|max| items.len() <= max);
    if !fits {
        return None;
    }
    let mut array = toml_edit::Array::new();
    for entry in items {
        array.push(item(entry)?);
    }
    Some(array.into())
}

fn new_child(inline: bool) -> toml_edit::Item {
    if inline {
        toml_edit::Item::Value(toml_edit::Value::InlineTable(toml_edit::InlineTable::new()))
    } else {
        let mut table = toml_edit::Table::new();
        table.set_implicit(true);
        toml_edit::Item::Table(table)
    }
}

fn write(
    item: &mut toml_edit::Item,
    parts: &[&str],
    value: Option<toml_edit::Value>,
) -> Result<(), ()> {
    let (key, rest) = parts.split_first().ok_or(())?;
    let inline = item.is_inline_table();
    let table = item.as_table_like_mut().ok_or(())?;
    if rest.is_empty() {
        set_leaf(table, key, value);
        return Ok(());
    }
    if !table.contains_key(key) {
        if value.is_none() {
            return Ok(());
        }
        table.insert(key, new_child(inline));
    }
    let removing = value.is_none();
    let child = table.get_mut(key).ok_or(())?;
    write(child, rest, value)?;
    let empty = removing && child.as_table_like().is_some_and(|child| child.is_empty());
    if empty {
        table.remove(key);
    }
    Ok(())
}

fn set_leaf(table: &mut dyn toml_edit::TableLike, key: &str, value: Option<toml_edit::Value>) {
    let Some(mut value) = value else {
        table.remove(key);
        return;
    };
    match table.get_mut(key) {
        Some(toml_edit::Item::Value(previous)) => {
            *value.decor_mut() = previous.decor().clone();
            *previous = value;
        }
        _ => {
            table.insert(key, toml_edit::Item::Value(value));
        }
    }
}
