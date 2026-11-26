//! The schema boundary for persisted snapshots, backups and peer updates.
//! Schema-less documents use the same layout, but may omit item, quality,
//! need and acquired; their historical defaults are retained without rewriting.
use loro::{Container, ContainerID, ContainerType, LoroDoc, LoroValue, ValueOrContainer};

use crate::{
    DocError, RowKey,
    document::{MAX_CRAFT_CHOICES, MAX_TARGETS, SCHEMA_VERSION, SCHEMA_VERSION_CRAFT},
    key::{CraftChoiceKey, TargetKey},
    snapshot::parse_scope,
};

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

fn boolean(value: Option<ValueOrContainer>) -> Option<bool> {
    match value {
        Some(ValueOrContainer::Value(LoroValue::Bool(value))) => Some(value),
        _ => None,
    }
}

fn valid_counter(value: Option<&ValueOrContainer>) -> bool {
    match value {
        Some(ValueOrContainer::Container(Container::Counter(counter))) => {
            let value = counter.get_value();
            value.is_finite()
                && value.fract() == 0.0
                && value >= i64::MIN as f64
                && value <= i64::MAX as f64
        }
        _ => false,
    }
}

pub(crate) fn validate(doc: &LoroDoc) -> Result<(), DocError> {
    // Inspect the original root types before get_map can create an empty map
    // alongside a malformed root of another type with the same name.
    let LoroValue::Map(roots) = doc.get_value() else {
        return Err(invalid("root"));
    };
    let meta = doc.get_map("meta");
    let legacy = meta.get("schema").is_none();
    let schema = if legacy {
        None
    } else {
        let schema = integer(meta.get("schema")).ok_or_else(|| invalid("meta.schema"))?;
        if schema != SCHEMA_VERSION && schema != SCHEMA_VERSION_CRAFT {
            return Err(DocError::UnsupportedSchema(schema));
        }
        Some(schema)
    };
    let is_craft_schema = schema == Some(SCHEMA_VERSION_CRAFT);

    for (name, value) in roots.iter() {
        let allowed_root = match name.as_str() {
            "meta" | "rows" => true,
            "targets" | "craft_choices" => is_craft_schema,
            _ => false,
        };
        if !allowed_root
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

    for field in meta.keys() {
        let allowed_meta = match field.as_str() {
            "schema" | "name" | "scope" => true,
            "exclude_crystals" | "include_vendors" => is_craft_schema,
            _ => false,
        };
        if !allowed_meta {
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
    for flag in ["exclude_crystals", "include_vendors"] {
        if meta.get(flag).is_some() && boolean(meta.get(flag)).is_none() {
            return Err(invalid(format!("meta.{flag}")));
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
            ref other if valid_counter(other.as_ref()) => {}
            _ => return Err(invalid(format!("{path}.acquired"))),
        }
    }

    if is_craft_schema {
        if roots.contains_key("targets") {
            let targets = doc.get_map("targets");
            let mut count_entries = 0_usize;
            for text in targets.keys() {
                count_entries = count_entries
                    .checked_add(1)
                    .filter(|&n| n <= MAX_TARGETS)
                    .ok_or_else(|| invalid("targets"))?;
                let path = format!("targets.{text}");
                let key: TargetKey = text.parse().map_err(|_| invalid(&path))?;
                let Some(ValueOrContainer::Container(Container::Map(target))) = targets.get(&text)
                else {
                    return Err(invalid(&path));
                };
                for field in target.keys() {
                    if !matches!(field.as_str(), "recipe" | "count" | "completed") {
                        return Err(invalid(format!("{path}.{field}")));
                    }
                }
                if integer(target.get("recipe")) != Some(i64::from(key.recipe_id)) {
                    return Err(invalid(format!("{path}.recipe")));
                }
                if !matches!(integer(target.get("count")), Some(c) if (1..=1_000_000).contains(&c))
                {
                    return Err(invalid(format!("{path}.count")));
                }
                if !valid_counter(target.get("completed").as_ref()) {
                    return Err(invalid(format!("{path}.completed")));
                }
            }
        }

        if roots.contains_key("craft_choices") {
            let choices = doc.get_map("craft_choices");
            let mut choice_entries = 0_usize;
            for text in choices.keys() {
                choice_entries = choice_entries
                    .checked_add(1)
                    .filter(|&n| n <= MAX_CRAFT_CHOICES)
                    .ok_or_else(|| invalid("craft_choices"))?;
                let path = format!("craft_choices.{text}");
                let key: CraftChoiceKey = text.parse().map_err(|_| invalid(&path))?;
                if key.item_id <= 0 || key.item_id > (i32::MAX - 2) / 4 {
                    return Err(invalid(&path));
                }
                let Some(ValueOrContainer::Container(Container::Map(choice))) = choices.get(&text)
                else {
                    return Err(invalid(&path));
                };
                for field in choice.keys() {
                    if !matches!(
                        field.as_str(),
                        "item" | "mode" | "recipe" | "on_hand" | "crafted"
                    ) {
                        return Err(invalid(format!("{path}.{field}")));
                    }
                }
                if integer(choice.get("item")) != Some(i64::from(key.item_id)) {
                    return Err(invalid(format!("{path}.item")));
                }
                if !matches!(string(choice.get("mode")).as_deref(), Some("craft" | "buy")) {
                    return Err(invalid(format!("{path}.mode")));
                }
                if choice.get("recipe").is_some()
                    && !matches!(
                        integer(choice.get("recipe")),
                        Some(r) if r > 0 && r <= i64::from(i32::MAX)
                    )
                {
                    return Err(invalid(format!("{path}.recipe")));
                }
                if !matches!(
                    integer(choice.get("on_hand")),
                    Some(oh) if (0..=1_000_000).contains(&oh)
                ) {
                    return Err(invalid(format!("{path}.on_hand")));
                }
                if !valid_counter(choice.get("crafted").as_ref()) {
                    return Err(invalid(format!("{path}.crafted")));
                }
            }
        }
    }

    Ok(())
}
