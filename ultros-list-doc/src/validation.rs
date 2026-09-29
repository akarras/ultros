//! The schema boundary for persisted snapshots, backups and peer updates.
//! Schema-less documents use the same layout, but may omit item, quality,
//! need and acquired; their historical defaults are retained without rewriting.
use loro::{Container, ContainerID, ContainerType, LoroDoc, LoroValue, ValueOrContainer};

use crate::{DocError, RowKey, document::SCHEMA_VERSION, snapshot::parse_scope};

fn invalid(path: impl Into<String>) -> DocError {
    DocError::InvalidStructure(path.into())
}

fn integer(value: Option<ValueOrContainer>) -> Option<i64> {
    match value {
        Some(ValueOrContainer::Value(LoroValue::I64(value))) => Some(value),
        _ => None,
    }
}

fn string(value: Option<ValueOrContainer>) -> Option<String> {
    match value {
        Some(ValueOrContainer::Value(LoroValue::String(value))) => Some(value.to_string()),
        _ => None,
    }
}

pub(crate) fn validate(doc: &LoroDoc) -> Result<(), DocError> {
    // Inspect the original root types before get_map can create an empty map
    // alongside a malformed root of another type with the same name.
    let LoroValue::Map(roots) = doc.get_value() else {
        return Err(invalid("root"));
    };
    for (name, value) in roots.iter() {
        if !matches!(name.as_str(), "meta" | "rows")
            || !matches!(
                value,
                LoroValue::Container(ContainerID::Root {
                    container_type: ContainerType::Map,
                    ..
                })
            )
        {
            return Err(invalid(format!("root.{name}")));
        }
    }
    let meta = doc.get_map("meta");
    let legacy = meta.get("schema").is_none();
    if !legacy {
        let schema = integer(meta.get("schema")).ok_or_else(|| invalid("meta.schema"))?;
        if schema != SCHEMA_VERSION {
            return Err(DocError::UnsupportedSchema(schema));
        }
    }
    for field in meta.keys() {
        if !matches!(field.as_str(), "schema" | "name" | "scope") {
            return Err(invalid(format!("meta.{field}")));
        }
    }
    if meta.get("name").is_some() && string(meta.get("name")).is_none() {
        return Err(invalid("meta.name"));
    }
    if meta.get("scope").is_some() {
        let scope = string(meta.get("scope")).ok_or_else(|| invalid("meta.scope"))?;
        let parsed = parse_scope(&scope).ok_or_else(|| invalid("meta.scope"))?;
        if crate::snapshot::encode_scope(parsed) != scope
            || scope
                .split_once(':')
                .is_none_or(|(_, id)| id.starts_with('-'))
        {
            return Err(invalid("meta.scope"));
        }
    }
    let rows = doc.get_map("rows");
    for text in rows.keys() {
        let path = format!("rows.{text}");
        let key: RowKey = text.parse().map_err(|_| invalid(&path))?;
        // The browser's reversible row id is item * 4 + quality (0..=2).
        if key.item_id <= 0 || key.item_id > (i32::MAX - 2) / 4 {
            return Err(invalid(&path));
        }
        let Some(ValueOrContainer::Container(Container::Map(row))) = rows.get(&text) else {
            return Err(invalid(&path));
        };
        for field in row.keys() {
            if !matches!(
                field.as_str(),
                "item" | "quality" | "need" | "target" | "acquired"
            ) {
                return Err(invalid(format!("{path}.{field}")));
            }
        }
        if (!legacy || row.get("item").is_some())
            && integer(row.get("item")) != Some(i64::from(key.item_id))
        {
            return Err(invalid(format!("{path}.item")));
        }
        if (!legacy || row.get("quality").is_some())
            && string(row.get("quality")).as_deref() != Some(key.quality.as_str())
        {
            return Err(invalid(format!("{path}.quality")));
        }
        for field in ["need", "target"] {
            if (row.get(field).is_some() || (!legacy && field == "need"))
                && integer(row.get(field)).is_none()
            {
                return Err(invalid(format!("{path}.{field}")));
            }
        }
        match row.get("acquired") {
            None if legacy => {}
            Some(ValueOrContainer::Container(Container::Counter(counter))) => {
                let value = counter.get_value();
                // Negative totals are valid after concurrent undo. Preserve
                // signed values; relational/display clamping remains separate.
                if !value.is_finite()
                    || value.fract() != 0.0
                    || value < i64::MIN as f64
                    || value > i64::MAX as f64
                {
                    return Err(invalid(format!("{path}.acquired")));
                }
            }
            _ => return Err(invalid(format!("{path}.acquired"))),
        }
    }
    Ok(())
}
