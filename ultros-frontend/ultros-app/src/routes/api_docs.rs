//! Native interactive API documentation route (`/api-docs`).
//!
//! Fetches the generated OpenAPI 3.1.0 specification from `/openapi.json`
//! (invoked in-process through `SsrApi` during SSR and via HTTP in the browser)
//! so the spec stays out of the WASM bundle while SSR still renders the full
//! reference in document order (`SsrMode::InOrder`).

use std::collections::{BTreeMap, BTreeSet};

use crate::api::{ApiExplorerResponse, execute_api_explorer_request, get_openapi_spec};
use crate::components::app_link::AppLink;
use crate::components::icon::Icon;
use crate::components::meta::{MetaCanonical, MetaDescription, MetaTitle};
use crate::components::skeleton::BoxSkeleton;
use crate::i18n::*;
use icondata as i;
use leptos::either::Either;
use leptos::prelude::*;
use percent_encoding::{AsciiSet, CONTROLS, NON_ALPHANUMERIC, utf8_percent_encode};
use serde_json::Value;

/// Characters percent-encoded when substituting path template parameters.
/// Keeps unreserved characters plus `,` (for comma-separated `itemids`) literal
/// so generated URLs and `curl` commands stay readable.
const PATH_PARAM_ENCODE_SET: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'/')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'`')
    .add(b'{')
    .add(b'}');

const PREVIEW_BODY_LIMIT: usize = 24 * 1024;
const EXPANDED_BODY_LIMIT: usize = 256 * 1024;

#[derive(Clone, Debug, PartialEq)]
struct OpenApiDoc {
    title: String,
    version: String,
    description: String,
    base_url: String,
    tags: Vec<TagDoc>,
    endpoints: Vec<EndpointDoc>,
    schemas: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TagDoc {
    name: String,
    description: String,
}

#[derive(Clone, Debug, PartialEq)]
struct EndpointDoc {
    id: String,
    method: String,
    is_websocket: bool,
    path: String,
    tag: String,
    summary: String,
    description: String,
    params: Vec<ParamDoc>,
    request_body: Option<RequestBodyDoc>,
    responses: Vec<ResponseDoc>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ParamDoc {
    name: String,
    location: String,
    required: bool,
    description: String,
    type_label: String,
    enum_values: Vec<String>,
    default_value: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
struct RequestBodyDoc {
    required: bool,
    description: String,
    content_type: String,
    schema: Option<Value>,
    example_json: String,
}

#[derive(Clone, Debug, PartialEq)]
struct ResponseDoc {
    status: String,
    description: String,
    content_type: Option<String>,
    schema: Option<Value>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PropertyRow {
    name: String,
    type_label: String,
    required: bool,
    description: String,
}

impl OpenApiDoc {
    fn from_value(spec: &Value) -> Result<Self, String> {
        let obj = spec
            .as_object()
            .ok_or_else(|| "OpenAPI response was not a JSON object".to_string())?;
        let info = obj.get("info").and_then(Value::as_object);
        let title = info
            .and_then(|m| m.get("title"))
            .and_then(Value::as_str)
            .unwrap_or("Ultros Market API")
            .to_string();
        let version = info
            .and_then(|m| m.get("version"))
            .and_then(Value::as_str)
            .unwrap_or("v1")
            .to_string();
        let description = info
            .and_then(|m| m.get("description"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let base_url = obj
            .get("servers")
            .and_then(Value::as_array)
            .and_then(|arr| arr.first())
            .and_then(|srv| srv.get("url"))
            .and_then(Value::as_str)
            .unwrap_or("https://ultros.app")
            .trim_end_matches('/')
            .to_string();

        let schemas: BTreeMap<String, Value> = obj
            .get("components")
            .and_then(|c| c.get("schemas"))
            .and_then(Value::as_object)
            .map(|map| map.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default();

        let mut tags: Vec<TagDoc> = obj
            .get("tags")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| {
                        let name = item.get("name")?.as_str()?.to_string();
                        let description = item
                            .get("description")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string();
                        Some(TagDoc { name, description })
                    })
                    .collect()
            })
            .unwrap_or_default();

        let mut endpoints = Vec::new();
        if let Some(paths) = obj.get("paths").and_then(Value::as_object) {
            for (path, path_item) in paths {
                let Some(path_obj) = path_item.as_object() else {
                    continue;
                };
                for method in ["get", "post", "put", "patch", "delete"] {
                    let Some(op) = path_obj.get(method).and_then(Value::as_object) else {
                        continue;
                    };
                    let method_upper = method.to_ascii_uppercase();
                    let tag = op
                        .get("tags")
                        .and_then(Value::as_array)
                        .and_then(|arr| arr.first())
                        .and_then(Value::as_str)
                        .unwrap_or("General")
                        .to_string();
                    if !tags.iter().any(|t| t.name == tag) {
                        tags.push(TagDoc {
                            name: tag.clone(),
                            description: String::new(),
                        });
                    }
                    let summary = op
                        .get("summary")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    let op_description = op
                        .get("description")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    let id = op
                        .get("operationId")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .unwrap_or_else(|| format!("{method}-{path}"));
                    let params = op
                        .get("parameters")
                        .and_then(Value::as_array)
                        .map(|arr| arr.iter().filter_map(parse_param).collect())
                        .unwrap_or_default();
                    let request_body = op
                        .get("requestBody")
                        .and_then(|rb| parse_request_body(path, rb, &schemas));
                    let responses = op
                        .get("responses")
                        .and_then(Value::as_object)
                        .map(|map| {
                            map.iter()
                                .map(|(status, val)| parse_response(status, val))
                                .collect()
                        })
                        .unwrap_or_default();
                    let is_websocket = path == "/api/v1/realtime/events";
                    endpoints.push(EndpointDoc {
                        id,
                        method: method_upper,
                        is_websocket,
                        path: path.clone(),
                        tag,
                        summary,
                        description: op_description,
                        params,
                        request_body,
                        responses,
                    });
                }
            }
        }

        // Preserve tag order from top-level `tags`, sorting endpoints cleanly within each tag.
        let tag_rank: BTreeMap<&str, usize> = tags
            .iter()
            .enumerate()
            .map(|(idx, t)| (t.name.as_str(), idx))
            .collect();
        endpoints.sort_by(|a, b| {
            let ra = tag_rank.get(a.tag.as_str()).copied().unwrap_or(usize::MAX);
            let rb = tag_rank.get(b.tag.as_str()).copied().unwrap_or(usize::MAX);
            ra.cmp(&rb)
                .then_with(|| a.path.cmp(&b.path))
                .then_with(|| a.method.cmp(&b.method))
        });

        Ok(Self {
            title,
            version,
            description,
            base_url,
            tags,
            endpoints,
            schemas,
        })
    }
}

fn parse_param(value: &Value) -> Option<ParamDoc> {
    let obj = value.as_object()?;
    let name = obj.get("name")?.as_str()?.to_string();
    let location = obj
        .get("in")
        .and_then(Value::as_str)
        .unwrap_or("query")
        .to_string();
    let required = obj
        .get("required")
        .and_then(Value::as_bool)
        .unwrap_or(location == "path");
    let description = obj
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let schema = obj.get("schema");
    let type_label = schema
        .map(summarize_schema_type)
        .unwrap_or_else(|| "string".to_string());
    let enum_values = schema
        .and_then(|s| s.get("enum"))
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| match v {
                    Value::String(s) => Some(s.clone()),
                    Value::Number(n) => Some(n.to_string()),
                    Value::Bool(b) => Some(b.to_string()),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    let default_value = schema.and_then(|s| s.get("default")).map(scalar_to_string);
    Some(ParamDoc {
        name,
        location,
        required,
        description,
        type_label,
        enum_values,
        default_value,
    })
}

fn parse_request_body(
    path: &str,
    value: &Value,
    schemas: &BTreeMap<String, Value>,
) -> Option<RequestBodyDoc> {
    let obj = value.as_object()?;
    let required = obj
        .get("required")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let description = obj
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let content = obj.get("content").and_then(Value::as_object)?;
    let (content_type, media) = content
        .get_key_value("application/json")
        .or_else(|| content.iter().next())?;
    let schema = media.get("schema").cloned();
    let example_json = default_request_body(path, schema.as_ref(), schemas);
    Some(RequestBodyDoc {
        required,
        description,
        content_type: content_type.clone(),
        schema,
        example_json,
    })
}

fn parse_response(status: &str, value: &Value) -> ResponseDoc {
    let description = value
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let (content_type, schema) = value
        .get("content")
        .and_then(Value::as_object)
        .and_then(|content| {
            content
                .get_key_value("application/json")
                .or_else(|| content.iter().next())
        })
        .map(|(ct, media)| (Some(ct.clone()), media.get("schema").cloned()))
        .unwrap_or((None, None));
    ResponseDoc {
        status: status.to_string(),
        description,
        content_type,
        schema,
    }
}

fn scalar_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".to_string(),
        other => other.to_string(),
    }
}

fn extract_ref_name(schema: &Value) -> Option<&str> {
    schema
        .get("$ref")
        .and_then(Value::as_str)
        .and_then(|r| r.strip_prefix("#/components/schemas/"))
}

fn summarize_schema_type(schema: &Value) -> String {
    if let Some(name) = extract_ref_name(schema) {
        return name.to_string();
    }
    if let Some(one_of) = schema.get("oneOf").and_then(Value::as_array) {
        let parts: Vec<String> = one_of.iter().map(summarize_schema_type).collect();
        return parts.join(" | ");
    }
    if let Some(any_of) = schema.get("anyOf").and_then(Value::as_array) {
        let parts: Vec<String> = any_of.iter().map(summarize_schema_type).collect();
        return parts.join(" | ");
    }
    if let Some(all_of) = schema.get("allOf").and_then(Value::as_array) {
        if all_of.len() == 1 {
            return summarize_schema_type(&all_of[0]);
        }
        let parts: Vec<String> = all_of.iter().map(summarize_schema_type).collect();
        return parts.join(" & ");
    }
    if let Some(type_val) = schema.get("type") {
        match type_val {
            Value::String(t) => return format_typed_schema(t, schema),
            Value::Array(arr) => {
                let mut non_null = Vec::new();
                let mut nullable = false;
                for item in arr {
                    if let Some(s) = item.as_str() {
                        if s == "null" {
                            nullable = true;
                        } else {
                            non_null.push(format_typed_schema(s, schema));
                        }
                    }
                }
                let base = if non_null.is_empty() {
                    "null".to_string()
                } else {
                    non_null.join(" | ")
                };
                return if nullable { format!("{base}?") } else { base };
            }
            _ => {}
        }
    }
    if schema.get("enum").is_some() {
        return "enum".to_string();
    }
    "any".to_string()
}

fn format_typed_schema(ty: &str, schema: &Value) -> String {
    match ty {
        "array" => {
            if let Some(prefix) = schema.get("prefixItems").and_then(Value::as_array) {
                let elems: Vec<String> = prefix.iter().map(summarize_schema_type).collect();
                return format!("({})", elems.join(", "));
            }
            if let Some(items) = schema.get("items") {
                return format!("{}[]", summarize_schema_type(items));
            }
            "array".to_string()
        }
        "object" => {
            if let Some(add_props) = schema.get("additionalProperties")
                && add_props.is_object()
            {
                return format!("map<string, {}>", summarize_schema_type(add_props));
            }
            "object".to_string()
        }
        "integer" | "number" | "string" => {
            if let Some(fmt) = schema.get("format").and_then(Value::as_str) {
                format!("{ty} ({fmt})")
            } else {
                ty.to_string()
            }
        }
        other => other.to_string(),
    }
}

fn resolve_schema<'a>(
    schema: &'a Value,
    schemas: &'a BTreeMap<String, Value>,
    visited: &mut BTreeSet<String>,
) -> (&'a Value, Option<String>) {
    if let Some(name) = extract_ref_name(schema)
        && visited.insert(name.to_string())
        && let Some(target) = schemas.get(name)
    {
        return (target, Some(name.to_string()));
    }
    if let Some(all_of) = schema.get("allOf").and_then(Value::as_array)
        && all_of.len() == 1
    {
        return resolve_schema(&all_of[0], schemas, visited);
    }
    (schema, None)
}

fn extract_properties(schema: &Value) -> Vec<PropertyRow> {
    let Some(props) = schema.get("properties").and_then(Value::as_object) else {
        return Vec::new();
    };
    let required: BTreeSet<&str> = schema
        .get("required")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    props
        .iter()
        .map(|(name, prop_schema)| PropertyRow {
            name: name.clone(),
            type_label: summarize_schema_type(prop_schema),
            required: required.contains(name.as_str()),
            description: prop_schema
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        })
        .collect()
}

fn extract_enum_values(schema: &Value) -> Vec<String> {
    schema
        .get("enum")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().map(scalar_to_string).collect())
        .unwrap_or_default()
}

fn default_param_value(path: &str, param: &ParamDoc) -> String {
    if param.location == "path" {
        return match param.name.as_str() {
            "world" => "Gilgamesh".to_string(),
            "itemid" => "46010".to_string(),
            "itemids" => "46010,44178".to_string(),
            "query" => "Retainer".to_string(),
            "id" => "1".to_string(),
            _ => String::new(),
        };
    }
    if param.required {
        if path == "/api/v1/search" && param.name == "q" {
            return "Shamshir".to_string();
        }
        if let Some(first) = param.enum_values.first() {
            return first.clone();
        }
        if let Some(default) = &param.default_value {
            return default.clone();
        }
    }
    String::new()
}

fn default_request_body(
    path: &str,
    schema: Option<&Value>,
    schemas: &BTreeMap<String, Value>,
) -> String {
    match path {
        "/api/v1/floor_history/{world}" => {
            "{\n  \"item_ids\": [46010],\n  \"hq\": \"any\"\n}".to_string()
        }
        "/api/v1/sparklines/{world}" => "{\n  \"item_ids\": [46010]\n}".to_string(),
        "/api/v1/resale_quality/{world}" => {
            "{\n  \"item_ids\": [46010],\n  \"lookback_days\": 30\n}".to_string()
        }
        _ => {
            let Some(schema) = schema else {
                return "{}".to_string();
            };
            let mut visited = BTreeSet::new();
            let (resolved, _) = resolve_schema(schema, schemas, &mut visited);
            let mut obj = serde_json::Map::new();
            for prop in extract_properties(resolved) {
                if !prop.required {
                    continue;
                }
                let sample = if prop.type_label.ends_with("[]") {
                    Value::Array(vec![])
                } else if prop.type_label.starts_with("integer")
                    || prop.type_label.starts_with("number")
                {
                    Value::from(0)
                } else if prop.type_label.starts_with("boolean") {
                    Value::from(false)
                } else {
                    Value::String(String::new())
                };
                obj.insert(prop.name, sample);
            }
            serde_json::to_string_pretty(&Value::Object(obj)).unwrap_or_else(|_| "{}".to_string())
        }
    }
}

fn build_request_path(
    path_template: &str,
    params: &[ParamDoc],
    values: &BTreeMap<String, String>,
) -> Result<String, String> {
    let mut rendered = path_template.to_string();
    let mut query_pairs: Vec<(String, String)> = Vec::new();
    for param in params {
        let raw = values
            .get(&param.name)
            .map(|s| s.trim())
            .unwrap_or_default();
        if param.location == "path" {
            if raw.is_empty() {
                return Err(format!("Missing required path parameter `{}`", param.name));
            }
            let encoded = utf8_percent_encode(raw, PATH_PARAM_ENCODE_SET).to_string();
            rendered = rendered.replace(&format!("{{{}}}", param.name), &encoded);
        } else if param.location == "query" {
            if raw.is_empty() {
                if param.required {
                    return Err(format!("Missing required query parameter `{}`", param.name));
                }
                continue;
            }
            query_pairs.push((param.name.clone(), raw.to_string()));
        }
    }
    if !query_pairs.is_empty() {
        rendered.push('?');
        for (idx, (key, val)) in query_pairs.into_iter().enumerate() {
            if idx > 0 {
                rendered.push('&');
            }
            let enc_k = utf8_percent_encode(&key, NON_ALPHANUMERIC).to_string();
            let enc_v = utf8_percent_encode(&val, NON_ALPHANUMERIC).to_string();
            rendered.push_str(&enc_k);
            rendered.push('=');
            rendered.push_str(&enc_v);
        }
    }
    Ok(rendered)
}

fn shell_single_quote(input: &str) -> String {
    format!("'{}'", input.replace('\'', "'\\''"))
}

fn build_curl_command(
    base_url: &str,
    method: &str,
    request_path: &str,
    body: Option<&str>,
) -> String {
    let url = format!("{}{}", base_url.trim_end_matches('/'), request_path);
    if method == "POST" {
        let mut cmd = format!(
            "curl -s -X POST {} \\\n  -H 'Content-Type: application/json'",
            shell_single_quote(&url)
        );
        if let Some(body) = body.map(str::trim).filter(|s| !s.is_empty()) {
            let compact = serde_json::from_str::<Value>(body)
                .and_then(|v| serde_json::to_string(&v))
                .unwrap_or_else(|_| body.to_string());
            cmd.push_str(&format!(" \\\n  -d {}", shell_single_quote(&compact)));
        }
        cmd
    } else {
        format!("curl -s {}", shell_single_quote(&url))
    }
}

fn build_websocket_example(base_url: &str, path: &str) -> String {
    let ws_base = base_url
        .trim_end_matches('/')
        .replacen("https://", "wss://", 1)
        .replacen("http://", "ws://", 1);
    format!(
        "const ws = new WebSocket(\"{ws_base}{path}\");\n\
         ws.onopen = () => {{\n\
         \x20 ws.send(JSON.stringify({{\n\
         \x20   AddSubscribe: {{\n\
         \x20     socket_type: \"Listings\",\n\
         \x20     world_id: 63,\n\
         \x20     item_id: 46010\n\
         \x20   }}\n\
         \x20 }}));\n\
         }};\n\
         ws.onmessage = (event) => console.log(JSON.parse(event.data));"
    )
}

fn format_byte_size(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.2} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

fn truncate_utf8_prefix(s: &str, max_bytes: usize) -> (&str, bool) {
    if s.len() <= max_bytes {
        return (s, false);
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    (&s[..end], true)
}

fn tag_slug(name: &str) -> String {
    let mut slug = String::from("tag-");
    let mut prev_dash = true;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash {
            slug.push('-');
            prev_dash = true;
        }
    }
    slug.trim_end_matches('-').to_string()
}

fn method_badge_class(method: &str, is_websocket: bool) -> &'static str {
    if is_websocket {
        return "bg-purple-500/15 text-purple-300 border-purple-400/40";
    }
    match method {
        "GET" => "bg-emerald-500/15 text-emerald-300 border-emerald-400/40",
        "POST" => "bg-sky-500/15 text-sky-300 border-sky-400/40",
        "PUT" | "PATCH" => "bg-amber-500/15 text-amber-300 border-amber-400/40",
        "DELETE" => "bg-rose-500/15 text-rose-300 border-rose-400/40",
        _ => "bg-white/10 text-[color:var(--color-text)] border-[color:var(--color-outline)]",
    }
}

fn status_badge_class(status: &str) -> &'static str {
    if status.starts_with('2') || status == "101" {
        "bg-emerald-500/15 text-emerald-300 border-emerald-400/40"
    } else if status.starts_with('4') {
        "bg-amber-500/15 text-amber-300 border-amber-400/40"
    } else if status.starts_with('5') {
        "bg-rose-500/15 text-rose-300 border-rose-400/40"
    } else {
        "bg-white/10 text-[color:var(--color-text-muted)] border-[color:var(--color-outline)]"
    }
}

fn endpoint_matches_filter(
    endpoint: &EndpointDoc,
    query: &str,
    selected_tag: Option<&str>,
) -> bool {
    if let Some(tag) = selected_tag
        && endpoint.tag != tag
    {
        return false;
    }
    let q = query.trim().to_ascii_lowercase();
    if q.is_empty() {
        return true;
    }
    endpoint.path.to_ascii_lowercase().contains(&q)
        || endpoint.summary.to_ascii_lowercase().contains(&q)
        || endpoint.description.to_ascii_lowercase().contains(&q)
        || endpoint.method.to_ascii_lowercase().contains(&q)
        || endpoint.tag.to_ascii_lowercase().contains(&q)
        || endpoint
            .params
            .iter()
            .any(|p| p.name.to_ascii_lowercase().contains(&q))
}

#[component]
fn CopySnippetButton(
    #[prop(into)] text: Signal<String>,
    #[prop(optional, into)] label: Option<String>,
) -> impl IntoView {
    let i18n = use_i18n();
    let copied = RwSignal::new(false);
    let toasts = crate::global_state::toasts::use_toast();
    let label_text = label.unwrap_or_else(|| t_string!(i18n, api_docs_copy).to_string());

    let on_copy = move |_| {
        #[cfg(feature = "hydrate")]
        {
            use leptos::task::spawn_local;
            use wasm_bindgen_futures::JsFuture;
            if let Some(window) = web_sys::window() {
                let clipboard = window.navigator().clipboard();
                let value = text.get_untracked();
                let promise = clipboard.write_text(&value);
                let copied_msg = t_string!(i18n, api_docs_copied_toast).to_string();
                let failed_msg = t_string!(i18n, api_docs_copy_failed).to_string();
                spawn_local(async move {
                    if JsFuture::from(promise).await.is_ok() {
                        copied.set(true);
                        if let Some(toasts) = toasts {
                            toasts.success(copied_msg);
                        }
                        gloo_timers::future::TimeoutFuture::new(1500).await;
                        copied.set(false);
                    } else if let Some(toasts) = toasts {
                        toasts.error(failed_msg);
                    }
                });
            }
        }
        #[cfg(not(feature = "hydrate"))]
        {
            let _ = (toasts, copied, text);
        }
    };

    view! {
        <button
            type="button"
            class="btn-secondary text-xs px-2.5 py-1 inline-flex items-center gap-1.5 shrink-0"
            on:click=on_copy
        >
            <Icon
                icon=Signal::derive(move || {
                    if copied.get() {
                        i::BsClipboard2CheckFill
                    } else {
                        i::BsClipboard2Fill
                    }
                })
                width="0.95em"
                height="0.95em"
                aria_hidden=true
            />
            <span>{move || if copied.get() { t_string!(i18n, api_docs_copied).to_string() } else { label_text.clone() }}</span>
        </button>
    }
}

#[component]
fn PathDisplay(path: String) -> impl IntoView {
    // Split `/api/v1/listings/{world}/{itemid}` into plain segments and `{param}` chips.
    let mut parts: Vec<(bool, String)> = Vec::new();
    let mut rest = path.as_str();
    while let Some(start) = rest.find('{') {
        if start > 0 {
            parts.push((false, rest[..start].to_string()));
        }
        let after_start = &rest[start..];
        if let Some(end) = after_start.find('}') {
            parts.push((true, after_start[..=end].to_string()));
            rest = &after_start[end + 1..];
        } else {
            parts.push((false, after_start.to_string()));
            rest = "";
            break;
        }
    }
    if !rest.is_empty() {
        parts.push((false, rest.to_string()));
    }

    view! {
        <code class="font-mono text-xs sm:text-sm break-all text-[color:var(--color-text)]">
            {parts
                .into_iter()
                .map(|(is_param, segment)| {
                    if is_param {
                        Either::Left(view! {
                            <span class="text-brand-300 font-semibold">{segment}</span>
                        })
                    } else {
                        Either::Right(view! { <span>{segment}</span> })
                    }
                })
                .collect_view()}
        </code>
    }
}

#[component]
fn SchemaView(
    schema: Value,
    schemas: StoredValue<BTreeMap<String, Value>>,
    #[prop(default = 0)] depth: usize,
) -> impl IntoView {
    let i18n = use_i18n();
    let type_label = summarize_schema_type(&schema);
    let raw_json = serde_json::to_string_pretty(&schema).unwrap_or_else(|_| "{}".to_string());

    let detail = schemas.with_value(|map| {
        let mut visited = BTreeSet::new();
        let (resolved, ref_name) = resolve_schema(&schema, map, &mut visited);
        let description = resolved
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let properties = extract_properties(resolved);
        let enum_values = extract_enum_values(resolved);
        let variants: Vec<Value> = resolved
            .get("oneOf")
            .or_else(|| resolved.get("anyOf"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let item_schema = if resolved.get("type").and_then(Value::as_str) == Some("array") {
            resolved.get("items").cloned()
        } else {
            None
        };
        let add_props_schema = resolved
            .get("additionalProperties")
            .filter(|v| v.is_object())
            .cloned();
        (
            ref_name,
            description,
            properties,
            enum_values,
            variants,
            item_schema,
            add_props_schema,
        )
    });

    let (ref_name, description, properties, enum_values, variants, item_schema, add_props_schema) =
        detail;

    view! {
        <div class="space-y-2.5 text-xs min-w-0">
            <div class="flex flex-wrap items-center gap-2">
                <code class="font-mono text-brand-300 bg-white/5 border border-[color:var(--color-outline)] rounded px-2 py-0.5 break-all">
                    {type_label}
                </code>
                {ref_name.map(|name| view! {
                    <span class="text-[color:var(--color-text-muted)]">
                        {t!(i18n, api_docs_schema_ref, name = move || view! { <code class="font-mono">{name.clone()}</code> })}
                    </span>
                })}
            </div>

            {(!description.is_empty()).then(|| view! {
                <p class="text-[color:var(--color-text-muted)] leading-relaxed">{description}</p>
            })}

            {(!enum_values.is_empty()).then(|| view! {
                <div class="flex flex-wrap items-center gap-1.5">
                    <span class="text-[color:var(--color-text-muted)]">{t!(i18n, api_docs_values)}</span>
                    {enum_values
                        .into_iter()
                        .map(|val| view! {
                            <code class="font-mono px-1.5 py-0.5 rounded bg-black/30 border border-[color:var(--color-outline)] text-[color:var(--color-text)]">
                                {val}
                            </code>
                        })
                        .collect_view()}
                </div>
            })}

            {(!properties.is_empty()).then(|| view! {
                <div class="overflow-x-auto rounded-lg border border-[color:var(--color-outline)]">
                    <table class="w-full text-left border-collapse">
                        <thead>
                            <tr class="bg-black/25 text-[color:var(--color-text-muted)] border-b border-[color:var(--color-outline)]">
                                <th class="px-3 py-1.5 font-semibold">{t!(i18n, api_docs_field)}</th>
                                <th class="px-3 py-1.5 font-semibold">{t!(i18n, api_docs_type)}</th>
                                <th class="px-3 py-1.5 font-semibold">{t!(i18n, api_docs_description)}</th>
                            </tr>
                        </thead>
                        <tbody class="divide-y divide-[color:var(--color-outline)]/60">
                            {properties
                                .into_iter()
                                .map(|prop| view! {
                                    <tr class="align-top">
                                        <td class="px-3 py-1.5 font-mono whitespace-nowrap text-[color:var(--color-text)]">
                                            {prop.name}
                                            {prop.required.then(|| view! {
                                                <span class="ml-1 text-rose-300" title=t_string!(i18n, api_docs_required_field)>"*"</span>
                                            })}
                                        </td>
                                        <td class="px-3 py-1.5 font-mono text-brand-300 whitespace-nowrap">
                                            {prop.type_label}
                                        </td>
                                        <td class="px-3 py-1.5 text-[color:var(--color-text-muted)] min-w-[12rem]">
                                            {if prop.description.is_empty() { "—".to_string() } else { prop.description }}
                                        </td>
                                    </tr>
                                })
                                .collect_view()}
                        </tbody>
                    </table>
                </div>
            })}

            {(!variants.is_empty() && depth < 2).then(|| view! {
                <div class="space-y-2 pl-3 border-l-2 border-[color:var(--color-outline)]">
                    <div class="text-[color:var(--color-text-muted)] font-semibold uppercase tracking-wide text-[11px]">
                        {t!(i18n, api_docs_variants)}
                    </div>
                    {variants
                        .into_iter()
                        .map(|variant| view! {
                            <div class="rounded-lg bg-black/20 p-2.5 border border-[color:var(--color-outline)]/60">
                                <SchemaView schema=variant schemas depth=depth + 1 />
                            </div>
                        })
                        .collect_view()}
                </div>
            })}

            {item_schema.filter(|_| depth < 2).map(|item| view! {
                <div class="pl-3 border-l-2 border-[color:var(--color-outline)] space-y-1.5">
                    <div class="text-[color:var(--color-text-muted)] font-semibold uppercase tracking-wide text-[11px]">
                        {t!(i18n, api_docs_array_item)}
                    </div>
                    <SchemaView schema=item schemas depth=depth + 1 />
                </div>
            })}

            {add_props_schema.filter(|_| depth < 2).map(|val_schema| view! {
                <div class="pl-3 border-l-2 border-[color:var(--color-outline)] space-y-1.5">
                    <div class="text-[color:var(--color-text-muted)] font-semibold uppercase tracking-wide text-[11px]">
                        {t!(i18n, api_docs_map_value)}
                    </div>
                    <SchemaView schema=val_schema schemas depth=depth + 1 />
                </div>
            })}

            {(depth == 0).then(|| view! {
                <details class="group">
                    <summary class="cursor-pointer select-none text-[color:var(--color-text-muted)] hover:text-[color:var(--color-text)]">
                        {t!(i18n, api_docs_raw_schema_json)}
                    </summary>
                    <pre class="mt-2 p-3 rounded-lg bg-black/40 border border-[color:var(--color-outline)] overflow-x-auto text-[11px] font-mono text-[color:var(--color-text)] max-h-72">
                        <code>{raw_json}</code>
                    </pre>
                </details>
            })}
        </div>
    }.into_any()
}

#[component]
fn EndpointCard(
    endpoint: EndpointDoc,
    base_url: String,
    schemas: StoredValue<BTreeMap<String, Value>>,
) -> impl IntoView {
    let i18n = use_i18n();
    let expanded = RwSignal::new(false);
    let initial_params: BTreeMap<String, String> = endpoint
        .params
        .iter()
        .map(|p| (p.name.clone(), default_param_value(&endpoint.path, p)))
        .collect();
    let param_values = RwSignal::new(initial_params);
    let initial_body = endpoint
        .request_body
        .as_ref()
        .map(|rb| rb.example_json.clone())
        .unwrap_or_default();
    let request_body_text = RwSignal::new(initial_body);

    let running = RwSignal::new(false);
    let response_state: RwSignal<Option<Result<ApiExplorerResponse, String>>> = RwSignal::new(None);
    let show_full_body = RwSignal::new(false);

    let endpoint_stored = StoredValue::new(endpoint.clone());
    let base_url_stored = StoredValue::new(base_url.clone());

    let computed_path = Signal::derive(move || {
        endpoint_stored.with_value(|ep| {
            let vals = param_values.get();
            build_request_path(&ep.path, &ep.params, &vals)
        })
    });

    let snippet_text = Signal::derive(move || {
        endpoint_stored.with_value(|ep| {
            base_url_stored.with_value(|base| {
                if ep.is_websocket {
                    return build_websocket_example(base, &ep.path);
                }
                let path = computed_path.get().unwrap_or_else(|_| ep.path.clone());
                let body = if ep.method == "POST" {
                    Some(request_body_text.get())
                } else {
                    None
                };
                build_curl_command(base, &ep.method, &path, body.as_deref())
            })
        })
    });

    let send_request = move |_| {
        if running.get_untracked() {
            return;
        }
        let target = computed_path.get_untracked();
        let (method, has_body) =
            endpoint_stored.with_value(|ep| (ep.method.clone(), ep.request_body.is_some()));
        let body = has_body.then(|| request_body_text.get_untracked());

        match target {
            Err(err) => {
                response_state.set(Some(Err(err)));
            }
            Ok(path_and_query) => {
                if let Some(ref raw_body) = body
                    && !raw_body.trim().is_empty()
                    && let Err(parse_err) = serde_json::from_str::<Value>(raw_body)
                {
                    response_state.set(Some(Err(format!("Invalid JSON body: {parse_err}"))));
                    return;
                }
                running.set(true);
                show_full_body.set(false);
                leptos::task::spawn_local(async move {
                    let res =
                        execute_api_explorer_request(&method, &path_and_query, body.as_deref())
                            .await;
                    response_state.set(Some(res));
                    running.set(false);
                });
            }
        }
    };

    let method_label = if endpoint.is_websocket {
        "WS".to_string()
    } else {
        endpoint.method.clone()
    };
    let badge_cls = method_badge_class(&endpoint.method, endpoint.is_websocket);

    view! {
        <article
            id=endpoint.id.clone()
            class="panel rounded-xl overflow-hidden border border-[color:var(--color-outline)] min-w-0"
        >
            <button
                type="button"
                class="w-full text-left px-4 py-3.5 flex items-start sm:items-center gap-3 hover:bg-white/[0.03] transition-colors cursor-pointer min-w-0"
                aria-expanded=move || if expanded.get() { "true" } else { "false" }
                on:click=move |_| expanded.update(|open| *open = !*open)
            >
                <span class=format!(
                    "inline-flex items-center justify-center font-mono text-[11px] font-bold uppercase tracking-wider px-2 py-0.5 rounded border shrink-0 min-w-[3.25rem] {}",
                    badge_cls
                )>
                    {method_label}
                </span>
                <div class="min-w-0 flex-1 flex flex-col sm:flex-row sm:items-center gap-1 sm:gap-3">
                    <PathDisplay path=endpoint.path.clone() />
                    {(!endpoint.summary.is_empty()).then(|| view! {
                        <span class="text-xs sm:text-sm text-[color:var(--color-text-muted)] line-clamp-2 sm:line-clamp-1">
                            {endpoint.summary.clone()}
                        </span>
                    })}
                </div>
                <span class="shrink-0 text-[color:var(--color-text-muted)] mt-0.5 sm:mt-0">
                    <Icon
                        icon=Signal::derive(move || {
                            if expanded.get() {
                                i::BsChevronDown
                            } else {
                                i::BsChevronRight
                            }
                        })
                        width="1em"
                        height="1em"
                        aria_hidden=true
                    />
                </span>
            </button>

            <Show when=move || expanded.get()>
                {move || {
                    let ep = endpoint_stored.get_value();
                    view! {
                        <div class="border-t border-[color:var(--color-outline)] px-4 sm:px-5 py-5 space-y-6 bg-black/15 min-w-0">
                            {(!ep.description.is_empty()).then(|| view! {
                                <p class="text-sm text-[color:var(--color-text)] leading-relaxed whitespace-pre-line">
                                    {ep.description.clone()}
                                </p>
                            })}

                            // Parameters section
                            {(!ep.params.is_empty()).then(|| view! {
                                <section class="space-y-2.5 min-w-0">
                                    <h4 class="text-xs font-bold uppercase tracking-wider text-brand-300">
                                        {t!(i18n, api_docs_parameters)}
                                    </h4>
                                    <div class="overflow-x-auto rounded-lg border border-[color:var(--color-outline)]">
                                        <table class="w-full text-left border-collapse text-xs sm:text-sm">
                                            <thead>
                                                <tr class="bg-black/25 text-[color:var(--color-text-muted)] border-b border-[color:var(--color-outline)]">
                                                    <th class="px-3 py-2 font-semibold">{t!(i18n, api_docs_name)}</th>
                                                    <th class="px-3 py-2 font-semibold">{t!(i18n, api_docs_in)}</th>
                                                    <th class="px-3 py-2 font-semibold">{t!(i18n, api_docs_type)}</th>
                                                    <th class="px-3 py-2 font-semibold">{t!(i18n, api_docs_description)}</th>
                                                </tr>
                                            </thead>
                                            <tbody class="divide-y divide-[color:var(--color-outline)]/60">
                                                {ep.params.iter().map(|param| view! {
                                                    <tr class="align-top">
                                                        <td class="px-3 py-2 font-mono whitespace-nowrap text-[color:var(--color-text)]">
                                                            {param.name.clone()}
                                                            {param.required.then(|| view! {
                                                                <span class="ml-1.5 text-[10px] font-sans font-semibold uppercase px-1.5 py-0.5 rounded bg-rose-500/15 text-rose-300 border border-rose-400/30">
                                                                    {t!(i18n, api_docs_param_required_short)}
                                                                </span>
                                                            })}
                                                        </td>
                                                        <td class="px-3 py-2 font-mono text-xs text-[color:var(--color-text-muted)] whitespace-nowrap">
                                                            {param.location.clone()}
                                                        </td>
                                                        <td class="px-3 py-2 font-mono text-xs text-brand-300 whitespace-nowrap">
                                                            {param.type_label.clone()}
                                                        </td>
                                                        <td class="px-3 py-2 text-xs text-[color:var(--color-text-muted)] min-w-[14rem] space-y-1">
                                                            <div>
                                                                {if param.description.is_empty() { "—".to_string() } else { param.description.clone() }}
                                                            </div>
                                                            {(!param.enum_values.is_empty()).then(|| view! {
                                                                <div class="flex flex-wrap gap-1 pt-0.5">
                                                                    {param.enum_values.iter().map(|ev| view! {
                                                                        <code class="font-mono text-[11px] px-1.5 py-0.5 rounded bg-black/35 border border-[color:var(--color-outline)] text-[color:var(--color-text)]">
                                                                            {ev.clone()}
                                                                        </code>
                                                                    }).collect_view()}
                                                                </div>
                                                            })}
                                                        </td>
                                                    </tr>
                                                }).collect_view()}
                                            </tbody>
                                        </table>
                                    </div>
                                </section>
                            })}

                            // Request body schema (POST endpoints)
                            {ep.request_body.clone().map(|rb| view! {
                                <section class="space-y-2.5 min-w-0">
                                    <div class="flex flex-wrap items-center gap-2">
                                        <h4 class="text-xs font-bold uppercase tracking-wider text-brand-300">
                                            {t!(i18n, api_docs_request_body)}
                                        </h4>
                                        <code class="font-mono text-[11px] text-[color:var(--color-text-muted)]">
                                            {rb.content_type}
                                        </code>
                                        {rb.required.then(|| view! {
                                            <span class="text-[10px] font-semibold uppercase px-1.5 py-0.5 rounded bg-rose-500/15 text-rose-300 border border-rose-400/30">
                                                {t!(i18n, api_docs_required)}
                                            </span>
                                        })}
                                    </div>
                                    {(!rb.description.is_empty()).then(|| view! {
                                        <p class="text-xs text-[color:var(--color-text-muted)]">{rb.description}</p>
                                    })}
                                    {rb.schema.map(|s| view! {
                                        <div class="rounded-lg bg-black/25 border border-[color:var(--color-outline)] p-3.5">
                                            <SchemaView schema=s schemas />
                                        </div>
                                    })}
                                </section>
                            })}

                            // Interactive Try It Out + Code Example
                            <section class="space-y-3 rounded-xl bg-black/25 border border-[color:var(--color-outline)] p-4 min-w-0">
                                <div class="flex flex-wrap items-center justify-between gap-2">
                                    <h4 class="text-xs font-bold uppercase tracking-wider text-brand-300">
                                        {if ep.is_websocket {
                                            t_string!(i18n, api_docs_websocket_example).to_string()
                                        } else {
                                            t_string!(i18n, api_docs_try_it_out).to_string()
                                        }}
                                    </h4>
                                    <CopySnippetButton
                                        text=snippet_text
                                        label=if ep.is_websocket {
                                            t_string!(i18n, api_docs_copy_js).to_string()
                                        } else {
                                            t_string!(i18n, api_docs_copy_curl).to_string()
                                        }
                                    />
                                </div>

                                {if ep.is_websocket {
                                    Either::Left(view! {
                                        <div class="space-y-2">
                                            <p class="text-xs text-[color:var(--color-text-muted)]">
                                                {t!(i18n, api_docs_websocket_hint)}
                                            </p>
                                            <pre class="p-3 rounded-lg bg-black/45 border border-[color:var(--color-outline)] overflow-x-auto text-xs font-mono text-[color:var(--color-text)]">
                                                <code>{move || snippet_text.get()}</code>
                                            </pre>
                                        </div>
                                    })
                                } else {
                                    Either::Right(view! {
                                        <div class="space-y-3.5 min-w-0">
                                            {(!ep.params.is_empty()).then(|| view! {
                                                <div class="grid grid-cols-1 sm:grid-cols-2 gap-3">
                                                    {ep.params.iter().map(|param| {
                                                        let param_name = param.name.clone();
                                                        let param_name_input = param.name.clone();
                                                        let placeholder = if !param.enum_values.is_empty() {
                                                            param.enum_values.join(" | ")
                                                        } else if let Some(def) = &param.default_value {
                                                            t_string!(i18n, api_docs_param_default_placeholder, value = def.clone()).to_string()
                                                        } else {
                                                            param.type_label.clone()
                                                        };
                                                        let has_enum = !param.enum_values.is_empty();
                                                        let enum_vals = param.enum_values.clone();
                                                        let required = param.required;
                                                        view! {
                                                            <label class="flex flex-col gap-1 min-w-0">
                                                                <span class="text-xs font-mono text-[color:var(--color-text)] flex items-center gap-1.5">
                                                                    <span>{param.name.clone()}</span>
                                                                    <span class="text-[10px] text-[color:var(--color-text-muted)]">
                                                                        "(" {param.location.clone()} ")"
                                                                    </span>
                                                                    {required.then(|| view! {
                                                                        <span class="text-rose-300">"*"</span>
                                                                    })}
                                                                </span>
                                                                {if has_enum {
                                                                    Either::Left(view! {
                                                                        <select
                                                                            class="input text-xs font-mono py-1.5 px-2.5 w-full"
                                                                            prop:value=move || {
                                                                                param_values.with(|m| m.get(&param_name).cloned().unwrap_or_default())
                                                                            }
                                                                            on:change=move |ev| {
                                                                                let val = event_target_value(&ev);
                                                                                param_values.update(|m| {
                                                                                    m.insert(param_name_input.clone(), val);
                                                                                });
                                                                            }
                                                                        >
                                                                            {(!required).then(|| view! {
                                                                                <option value="">{t!(i18n, api_docs_param_default_option)}</option>
                                                                            })}
                                                                            {enum_vals.into_iter().map(|opt| {
                                                                                let val = opt.clone();
                                                                                view! {
                                                                                    <option value=val>{opt}</option>
                                                                                }
                                                                            }).collect_view()}
                                                                        </select>
                                                                    })
                                                                } else {
                                                                    Either::Right(view! {
                                                                        <input
                                                                            type="text"
                                                                            class="input text-xs font-mono py-1.5 px-2.5 w-full"
                                                                            placeholder=placeholder
                                                                            prop:value=move || {
                                                                                param_values.with(|m| m.get(&param_name).cloned().unwrap_or_default())
                                                                            }
                                                                            on:input=move |ev| {
                                                                                let val = event_target_value(&ev);
                                                                                param_values.update(|m| {
                                                                                    m.insert(param_name_input.clone(), val);
                                                                                });
                                                                            }
                                                                        />
                                                                    })
                                                                }}
                                                            </label>
                                                        }
                                                    }).collect_view()}
                                                </div>
                                            })}

                                            {ep.request_body.is_some().then(|| view! {
                                                <label class="flex flex-col gap-1 min-w-0">
                                                    <span class="text-xs font-mono text-[color:var(--color-text)]">
                                                        {t!(i18n, api_docs_json_request_body)}
                                                    </span>
                                                    <textarea
                                                        rows="4"
                                                        class="input text-xs font-mono p-2.5 w-full leading-relaxed"
                                                        prop:value=move || request_body_text.get()
                                                        on:input=move |ev| request_body_text.set(event_target_value(&ev))
                                                    />
                                                </label>
                                            })}

                                            <pre class="p-3 rounded-lg bg-black/45 border border-[color:var(--color-outline)] overflow-x-auto text-xs font-mono text-[color:var(--color-text)]">
                                                <code>{move || snippet_text.get()}</code>
                                            </pre>

                                            <div class="flex flex-wrap items-center gap-3">
                                                <button
                                                    type="button"
                                                    class="btn-primary text-xs sm:text-sm px-3.5 py-1.5 inline-flex items-center gap-2"
                                                    disabled=move || running.get()
                                                    on:click=send_request
                                                >
                                                    <Icon icon=i::BsPlayFill width="1.1em" height="1.1em" aria_hidden=true />
                                                    <span>{move || if running.get() {
                                                        t_string!(i18n, api_docs_sending).to_string()
                                                    } else {
                                                        t_string!(i18n, api_docs_send_request).to_string()
                                                    }}</span>
                                                </button>
                                                {move || {
                                                    computed_path.get().ok().map(|p| view! {
                                                        <code class="text-xs font-mono text-[color:var(--color-text-muted)] break-all">
                                                            {p}
                                                        </code>
                                                    })
                                                }}
                                            </div>

                                            // Live Response Viewer
                                            {move || {
                                                response_state.get().map(|res| match res {
                                                    Err(err) => Either::Left(view! {
                                                        <div class="rounded-lg border border-rose-400/40 bg-rose-500/10 p-3 text-xs text-rose-200">
                                                            {err}
                                                        </div>
                                                    }),
                                                    Ok(resp) => {
                                                        let status_str = resp.status.to_string();
                                                        let status_cls = status_badge_class(&status_str);
                                                        let size_label = format_byte_size(resp.body.len());
                                                        let formatted = serde_json::from_str::<Value>(&resp.body)
                                                            .and_then(|v| serde_json::to_string_pretty(&v))
                                                            .unwrap_or_else(|_| resp.body.clone());
                                                        let full_len = formatted.len();
                                                        let copy_signal = Signal::derive({
                                                            let full = formatted.clone();
                                                            move || full.clone()
                                                        });
                                                        let formatted_stored = StoredValue::new(formatted);
                                                        Either::Right(view! {
                                                            <div class="space-y-2 rounded-lg border border-[color:var(--color-outline)] bg-black/40 p-3 min-w-0">
                                                                <div class="flex flex-wrap items-center justify-between gap-2 text-xs">
                                                                    <div class="flex flex-wrap items-center gap-2">
                                                                        <span class=format!("font-mono font-bold px-2 py-0.5 rounded border {}", status_cls)>
                                                                            "HTTP " {resp.status}
                                                                        </span>
                                                                        <span class="text-[color:var(--color-text-muted)] tabular-nums">
                                                                            {resp.elapsed_ms} " ms"
                                                                        </span>
                                                                        <span class="text-[color:var(--color-text-muted)] tabular-nums">
                                                                            {size_label}
                                                                        </span>
                                                                        <code class="font-mono text-[11px] text-[color:var(--color-text-muted)]">
                                                                            {resp.content_type}
                                                                        </code>
                                                                    </div>
                                                                    <CopySnippetButton text=copy_signal label=t_string!(i18n, api_docs_copy_response).to_string() />
                                                                </div>

                                                                {move || {
                                                                    let expanded_body = show_full_body.get();
                                                                    let limit = if expanded_body { EXPANDED_BODY_LIMIT } else { PREVIEW_BODY_LIMIT };
                                                                    let (slice, was_truncated) = formatted_stored.with_value(|body| {
                                                                        let (prefix, trunc) = truncate_utf8_prefix(body, limit);
                                                                        (prefix.to_string(), trunc)
                                                                    });
                                                                    view! {
                                                                        <div class="space-y-2 min-w-0">
                                                                            <pre class="p-3 rounded-lg bg-black/50 border border-[color:var(--color-outline)]/60 overflow-x-auto text-[11px] sm:text-xs font-mono text-[color:var(--color-text)] max-h-96">
                                                                                <code>{slice}</code>
                                                                            </pre>
                                                                            {was_truncated.then(|| view! {
                                                                                <div class="flex flex-wrap items-center justify-between gap-2 text-xs text-[color:var(--color-text-muted)]">
                                                                                    <span>
                                                                                        {t_string!(
                                                                                            i18n,
                                                                                            api_docs_truncated_notice,
                                                                                            shown = format_byte_size(limit),
                                                                                            total = format_byte_size(full_len)
                                                                                        ).to_string()}
                                                                                    </span>
                                                                                    {(!expanded_body).then(|| view! {
                                                                                        <button
                                                                                            type="button"
                                                                                            class="btn-ghost text-xs px-2 py-1"
                                                                                            on:click=move |_| show_full_body.set(true)
                                                                                        >
                                                                                            {t!(i18n, api_docs_show_more)}
                                                                                        </button>
                                                                                    })}
                                                                                </div>
                                                                            })}
                                                                        </div>
                                                                    }
                                                                }}
                                                            </div>
                                                        })
                                                    }
                                                })
                                            }}
                                        </div>
                                    })
                                }}
                            </section>

                            // Responses section
                            {(!ep.responses.is_empty()).then(|| view! {
                                <section class="space-y-2.5 min-w-0">
                                    <h4 class="text-xs font-bold uppercase tracking-wider text-brand-300">
                                        {t!(i18n, api_docs_responses)}
                                    </h4>
                                    <div class="space-y-3">
                                        {ep.responses.into_iter().map(|resp| {
                                            let badge = status_badge_class(&resp.status);
                                            view! {
                                                <div class="rounded-lg bg-black/25 border border-[color:var(--color-outline)] p-3.5 space-y-2.5 min-w-0">
                                                    <div class="flex flex-wrap items-center gap-2 text-xs sm:text-sm">
                                                        <span class=format!("font-mono font-bold px-2 py-0.5 rounded border {}", badge)>
                                                            {resp.status}
                                                        </span>
                                                        <span class="text-[color:var(--color-text)]">
                                                            {resp.description}
                                                        </span>
                                                        {resp.content_type.map(|ct| view! {
                                                            <code class="font-mono text-[11px] text-[color:var(--color-text-muted)]">
                                                                {ct}
                                                            </code>
                                                        })}
                                                    </div>
                                                    {resp.schema.map(|s| view! {
                                                        <SchemaView schema=s schemas />
                                                    })}
                                                </div>
                                            }
                                        }).collect_view()}
                                    </div>
                                </section>
                            })}
                        </div>
                    }
                }}
            </Show>
        </article>
    }
}

#[component]
fn SchemasDirectory(schemas: StoredValue<BTreeMap<String, Value>>) -> impl IntoView {
    let i18n = use_i18n();
    let schema_filter = RwSignal::new(String::new());
    let total_count = schemas.with_value(BTreeMap::len);

    view! {
        <section id="schemas" class="panel rounded-2xl p-5 sm:p-6 space-y-4 min-w-0">
            <div class="flex flex-col sm:flex-row sm:items-center justify-between gap-3">
                <div>
                    <h2 class="text-xl font-bold text-[color:var(--brand-fg)]">
                        {t!(i18n, api_docs_data_models)}
                    </h2>
                    <p class="text-xs sm:text-sm text-[color:var(--color-text-muted)]">
                        {t_string!(i18n, api_docs_schema_count, count = total_count).to_string()}
                    </p>
                </div>
                <input
                    type="search"
                    class="input text-xs sm:text-sm py-1.5 px-3 sm:w-64"
                    placeholder=t_string!(i18n, api_docs_filter_schemas_placeholder)
                    aria-label=t_string!(i18n, api_docs_filter_schemas_label)
                    prop:value=move || schema_filter.get()
                    on:input=move |ev| schema_filter.set(event_target_value(&ev))
                />
            </div>

            <div class="space-y-2">
                {move || {
                    let needle = schema_filter.get().trim().to_ascii_lowercase();
                    let entries: Vec<(String, Value)> = schemas.with_value(|map| {
                        map.iter()
                            .filter(|(name, _)| {
                                needle.is_empty() || name.to_ascii_lowercase().contains(&needle)
                            })
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect()
                    });
                    if entries.is_empty() {
                        Either::Left(view! {
                            <p class="text-sm text-[color:var(--color-text-muted)] py-2">
                                {t!(i18n, api_docs_no_schemas_match)}
                            </p>
                        })
                    } else {
                        Either::Right(
                            entries
                                .into_iter()
                                .map(|(name, schema)| {
                                    let summary_type = summarize_schema_type(&schema);
                                    view! {
                                        <details class="rounded-xl bg-black/20 border border-[color:var(--color-outline)] overflow-hidden">
                                            <summary class="cursor-pointer select-none px-4 py-2.5 flex flex-wrap items-center justify-between gap-2 hover:bg-white/[0.03]">
                                                <code class="font-mono text-xs sm:text-sm font-semibold text-[color:var(--brand-fg)]">
                                                    {name}
                                                </code>
                                                <code class="font-mono text-[11px] text-[color:var(--color-text-muted)]">
                                                    {summary_type}
                                                </code>
                                            </summary>
                                            <div class="border-t border-[color:var(--color-outline)] px-4 py-3.5 bg-black/15">
                                                <SchemaView schema schemas />
                                            </div>
                                        </details>
                                    }
                                })
                                .collect_view(),
                        )
                    }
                }}
            </div>
        </section>
    }
}

#[component]
pub fn ApiDocs() -> impl IntoView {
    let i18n = use_i18n();
    let spec_resource = Resource::new_blocking(|| (), |()| get_openapi_spec());
    let search_query = RwSignal::new(String::new());
    let selected_tag: RwSignal<Option<String>> = RwSignal::new(None);

    view! {
        <MetaCanonical href="https://ultros.app/api-docs" />
        <MetaTitle title=t_string!(i18n, api_docs_meta_title).to_string() />
        <MetaDescription text=t_string!(i18n, api_docs_meta_desc).to_string() />
        <div class="main-content p-2 sm:p-6">
            <div class="container mx-auto max-w-5xl flex flex-col gap-6 min-w-0">
                <Suspense fallback=move || view! { <BoxSkeleton rows=10 /> }>
                    {move || {
                        spec_resource.get().map(|result| match result.and_then(|v| {
                            OpenApiDoc::from_value(&v)
                                .map_err(crate::error::AppError::Json)
                        }) {
                            Ok(doc) => {
                                let schemas = StoredValue::new(doc.schemas.clone());
                                let endpoint_count = doc.endpoints.len();
                                let schema_count = doc.schemas.len();
                                let tags = doc.tags.clone();
                                let endpoints = StoredValue::new(doc.endpoints.clone());
                                let base_url = doc.base_url.clone();

                                Either::Left(view! {
                                    // Hero card
                                    <section class="panel p-6 sm:p-8 rounded-2xl space-y-5 min-w-0">
                                        <div class="flex flex-wrap items-center gap-2 text-xs">
                                            <span class="font-mono font-semibold px-2.5 py-0.5 rounded-full bg-brand-300/15 text-brand-300 border border-brand-300/35">
                                                "OpenAPI 3.1"
                                            </span>
                                            <span class="font-mono px-2.5 py-0.5 rounded-full bg-white/5 text-[color:var(--color-text-muted)] border border-[color:var(--color-outline)]">
                                                {doc.version.clone()}
                                            </span>
                                            <span class="px-2.5 py-0.5 rounded-full bg-emerald-500/10 text-emerald-300 border border-emerald-400/30">
                                                {t!(i18n, api_docs_no_auth)}
                                            </span>
                                        </div>

                                        <div class="space-y-2">
                                            <h1 class="text-3xl sm:text-4xl font-bold text-[color:var(--brand-fg)]">
                                                {doc.title.clone()}
                                            </h1>
                                            <p class="text-sm sm:text-base text-[color:var(--color-text)] leading-relaxed max-w-3xl">
                                                {doc.description.clone()}
                                            </p>
                                        </div>

                                        <div class="flex flex-wrap items-center gap-2.5 pt-1">
                                            <a
                                                href="/openapi.json"
                                                target="_blank"
                                                rel="noopener noreferrer"
                                                class="btn-primary text-xs sm:text-sm px-3.5 py-2 inline-flex items-center gap-2"
                                            >
                                                <Icon icon=i::BsCodeSlash width="1.1em" height="1.1em" aria_hidden=true />
                                                <span>{t!(i18n, api_docs_download_json)}</span>
                                            </a>
                                            <AppLink
                                                href="/help/ai-agents"
                                                attr:class="btn-secondary text-xs sm:text-sm px-3.5 py-2 inline-flex items-center gap-2"
                                            >
                                                <span>{t!(i18n, api_docs_mcp_agents)}</span>
                                                <Icon icon=i::FaArrowRightSolid width="0.8em" height="0.8em" aria_hidden=true />
                                            </AppLink>
                                            <a
                                                href="/llms.txt"
                                                target="_blank"
                                                rel="noopener noreferrer"
                                                class="btn-ghost text-xs sm:text-sm px-3 py-2"
                                            >
                                                "llms.txt"
                                            </a>
                                            <a
                                                href="#schemas"
                                                class="btn-ghost text-xs sm:text-sm px-3 py-2"
                                            >
                                                {t_string!(i18n, api_docs_data_models_count, count = schema_count).to_string()}
                                            </a>
                                        </div>

                                        <div class="grid grid-cols-1 sm:grid-cols-3 gap-3 pt-2 border-t border-[color:var(--color-outline)]/70 text-xs">
                                            <div class="rounded-xl bg-black/20 border border-[color:var(--color-outline)]/70 p-3 space-y-1">
                                                <div class="font-semibold text-[color:var(--brand-fg)]">{t!(i18n, api_docs_base_url)}</div>
                                                <code class="font-mono text-brand-300 break-all">{base_url.clone()}</code>
                                            </div>
                                            <div class="rounded-xl bg-black/20 border border-[color:var(--color-outline)]/70 p-3 space-y-1">
                                                <div class="font-semibold text-[color:var(--brand-fg)]">{t_string!(i18n, api_docs_world_scope, param = "{world}").to_string()}</div>
                                                <p class="text-[color:var(--color-text-muted)]">
                                                    {t!(
                                                        i18n,
                                                        api_docs_world_scope_hint,
                                                        world = || view! { <code class="font-mono">"Gilgamesh"</code> },
                                                        datacenter = || view! { <code class="font-mono">"Aether"</code> },
                                                        region = || view! { <code class="font-mono">"North-America"</code> }
                                                    )}
                                                </p>
                                            </div>
                                            <div class="rounded-xl bg-black/20 border border-[color:var(--color-outline)]/70 p-3 space-y-1">
                                                <div class="font-semibold text-[color:var(--brand-fg)]">{t!(i18n, api_docs_bulk_caching)}</div>
                                                <p class="text-[color:var(--color-text-muted)]">
                                                    {t!(
                                                        i18n,
                                                        api_docs_bulk_caching_hint,
                                                        format = || view! { <code class="font-mono">"?format=columnar"</code> },
                                                        header = || view! { <code class="font-mono">"Cache-Control"</code> }
                                                    )}
                                                </p>
                                            </div>
                                        </div>
                                    </section>

                                    // Search + Tag Filter Bar
                                    <section class="panel rounded-2xl p-4 sm:p-5 space-y-3 min-w-0">
                                        <div class="flex flex-col sm:flex-row sm:items-center gap-3">
                                            <div class="relative flex-1 min-w-0">
                                                <input
                                                    type="search"
                                                    class="input w-full text-sm py-2 px-3.5"
                                                    placeholder=t_string!(i18n, api_docs_filter_endpoints_placeholder)
                                                    aria-label=t_string!(i18n, api_docs_filter_endpoints_label)
                                                    prop:value=move || search_query.get()
                                                    on:input=move |ev| search_query.set(event_target_value(&ev))
                                                />
                                            </div>
                                            <div class="text-xs text-[color:var(--color-text-muted)] whitespace-nowrap tabular-nums">
                                                {move || {
                                                    let q = search_query.get();
                                                    let active_tag = selected_tag.get();
                                                    let visible = endpoints.with_value(|eps| {
                                                        eps.iter()
                                                            .filter(|ep| endpoint_matches_filter(ep, &q, active_tag.as_deref()))
                                                            .count()
                                                    });
                                                    t_string!(i18n, api_docs_visible_endpoints, visible = visible, total = endpoint_count).to_string()
                                                }}
                                            </div>
                                        </div>

                                        <div class="flex flex-wrap gap-2">
                                            <button
                                                type="button"
                                                class=move || {
                                                    if selected_tag.get().is_none() {
                                                        "btn-primary text-xs px-3 py-1.5 rounded-full"
                                                    } else {
                                                        "btn-secondary text-xs px-3 py-1.5 rounded-full"
                                                    }
                                                }
                                                on:click=move |_| selected_tag.set(None)
                                            >
                                                {t_string!(i18n, api_docs_all_count, count = endpoint_count).to_string()}
                                            </button>
                                            {tags.iter().map(|tag| {
                                                let tag_name = tag.name.clone();
                                                let tag_name_active = tag.name.clone();
                                                let tag_name_click = tag.name.clone();
                                                let count = endpoints.with_value(|eps| {
                                                    eps.iter().filter(|e| e.tag == tag_name).count()
                                                });
                                                view! {
                                                    <button
                                                        type="button"
                                                        class=move || {
                                                            if selected_tag.get().as_deref() == Some(tag_name_active.as_str()) {
                                                                "btn-primary text-xs px-3 py-1.5 rounded-full"
                                                            } else {
                                                                "btn-secondary text-xs px-3 py-1.5 rounded-full"
                                                            }
                                                        }
                                                        on:click=move |_| {
                                                            let next = tag_name_click.clone();
                                                            selected_tag.update(|cur| {
                                                                if cur.as_deref() == Some(next.as_str()) {
                                                                    *cur = None;
                                                                } else {
                                                                    *cur = Some(next);
                                                                }
                                                            });
                                                        }
                                                    >
                                                        {tag.name.clone()} " (" {count} ")"
                                                    </button>
                                                }
                                            }).collect_view()}
                                        </div>
                                    </section>

                                    // Grouped Endpoints
                                    {move || {
                                        let q = search_query.get();
                                        let active_tag = selected_tag.get();
                                        let groups: Vec<(TagDoc, Vec<EndpointDoc>)> = tags
                                            .iter()
                                            .filter_map(|tag| {
                                                let matched: Vec<EndpointDoc> = endpoints.with_value(|eps| {
                                                    eps.iter()
                                                        .filter(|ep| ep.tag == tag.name && endpoint_matches_filter(ep, &q, active_tag.as_deref()))
                                                        .cloned()
                                                        .collect()
                                                });
                                                (!matched.is_empty()).then(|| (tag.clone(), matched))
                                            })
                                            .collect();

                                        if groups.is_empty() {
                                            Either::Left(view! {
                                                <div class="panel rounded-2xl p-8 text-center space-y-3">
                                                    <p class="text-sm text-[color:var(--color-text-muted)]">
                                                        {t!(i18n, api_docs_no_endpoints_match)}
                                                    </p>
                                                    <button
                                                        type="button"
                                                        class="btn-secondary text-xs px-3 py-1.5"
                                                        on:click=move |_| {
                                                            search_query.set(String::new());
                                                            selected_tag.set(None);
                                                        }
                                                    >
                                                        {t!(i18n, api_docs_reset_filters)}
                                                    </button>
                                                </div>
                                            })
                                        } else {
                                            Either::Right(
                                                groups
                                                    .into_iter()
                                                    .map(|(tag, eps)| {
                                                        let base = base_url.clone();
                                                        let section_id = tag_slug(&tag.name);
                                                        view! {
                                                            <section id=section_id class="space-y-3 scroll-mt-20 min-w-0">
                                                                <div class="space-y-1 px-1">
                                                                    <h2 class="text-xl font-bold text-[color:var(--brand-fg)]">
                                                                        {tag.name}
                                                                    </h2>
                                                                    {(!tag.description.is_empty()).then(|| view! {
                                                                        <p class="text-xs sm:text-sm text-[color:var(--color-text-muted)]">
                                                                            {tag.description}
                                                                        </p>
                                                                    })}
                                                                </div>
                                                                <div class="flex flex-col gap-3 min-w-0">
                                                                    {eps
                                                                        .into_iter()
                                                                        .map(|ep| view! {
                                                                            <EndpointCard
                                                                                endpoint=ep
                                                                                base_url=base.clone()
                                                                                schemas
                                                                            />
                                                                        })
                                                                        .collect_view()}
                                                                </div>
                                                            </section>
                                                        }
                                                    })
                                                    .collect_view(),
                                            )
                                        }
                                    }}

                                    <SchemasDirectory schemas />
                                })
                            }
                            Err(error) => Either::Right(view! {
                                <div class="panel rounded-2xl p-6 space-y-2 border border-rose-400/40">
                                    <h1 class="text-xl font-bold text-[color:var(--brand-fg)]">
                                        {t!(i18n, api_docs_load_error)}
                                    </h1>
                                    <p class="text-sm text-[color:var(--color-text-muted)]">
                                        {error.to_string()}
                                    </p>
                                    <a href="/openapi.json" class="text-sm text-brand-300 underline">
                                        {t!(i18n, api_docs_open_raw_json)}
                                    </a>
                                </div>
                            }),
                        })
                    }}
                </Suspense>
            </div>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_openapi_spec_into_groups_params_and_schemas() {
        let spec = json!({
            "openapi": "3.1.0",
            "info": {
                "title": "Ultros Market API",
                "version": "v1",
                "description": "Public FFXIV market endpoints."
            },
            "servers": [{ "url": "https://ultros.app" }],
            "tags": [
                { "name": "Market Data", "description": "Live listings" },
                { "name": "Worlds, Search & Metadata", "description": "Reference data" }
            ],
            "paths": {
                "/api/v1/listings/{world}/{itemid}": {
                    "get": {
                        "tags": ["Market Data"],
                        "operationId": "getItemListings",
                        "summary": "Get item listings",
                        "parameters": [
                            { "name": "world", "in": "path", "required": true, "schema": { "type": "string" } },
                            { "name": "itemid", "in": "path", "required": true, "schema": { "type": "integer", "format": "int32" } }
                        ],
                        "responses": {
                            "200": {
                                "description": "OK",
                                "content": {
                                    "application/json": {
                                        "schema": { "$ref": "#/components/schemas/CurrentlyShownItem" }
                                    }
                                }
                            }
                        }
                    }
                },
                "/api/v1/search": {
                    "get": {
                        "tags": ["Worlds, Search & Metadata"],
                        "operationId": "searchMarket",
                        "summary": "Search items and retainers",
                        "parameters": [
                            { "name": "q", "in": "query", "required": true, "schema": { "type": "string" } }
                        ],
                        "responses": {
                            "200": { "description": "OK" }
                        }
                    }
                }
            },
            "components": {
                "schemas": {
                    "CurrentlyShownItem": {
                        "type": "object",
                        "required": ["listings"],
                        "properties": {
                            "listings": { "type": "array", "items": { "type": "string" } }
                        }
                    }
                }
            }
        });

        let doc = OpenApiDoc::from_value(&spec).expect("valid OpenAPI doc");
        assert_eq!(doc.title, "Ultros Market API");
        assert_eq!(doc.base_url, "https://ultros.app");
        assert_eq!(doc.endpoints.len(), 2);
        assert_eq!(doc.endpoints[0].tag, "Market Data");
        assert_eq!(doc.endpoints[1].tag, "Worlds, Search & Metadata");
        assert_eq!(doc.endpoints[0].params.len(), 2);
        assert_eq!(doc.endpoints[0].params[1].type_label, "integer (int32)");
        assert!(doc.schemas.contains_key("CurrentlyShownItem"));
    }

    #[test]
    fn builds_request_path_and_curl_commands() {
        let params = vec![
            ParamDoc {
                name: "world".into(),
                location: "path".into(),
                required: true,
                description: String::new(),
                type_label: "string".into(),
                enum_values: vec![],
                default_value: None,
            },
            ParamDoc {
                name: "format".into(),
                location: "query".into(),
                required: false,
                description: String::new(),
                type_label: "string".into(),
                enum_values: vec!["rows".into(), "columnar".into()],
                default_value: None,
            },
        ];
        let mut values = BTreeMap::new();
        values.insert("world".into(), "North-America".into());
        values.insert("format".into(), "columnar".into());

        let path = build_request_path("/api/v1/cheapest/{world}", &params, &values).unwrap();
        assert_eq!(path, "/api/v1/cheapest/North-America?format=columnar");

        let get_curl = build_curl_command("https://ultros.app", "GET", &path, None);
        assert_eq!(
            get_curl,
            "curl -s 'https://ultros.app/api/v1/cheapest/North-America?format=columnar'"
        );

        let post_curl = build_curl_command(
            "https://ultros.app",
            "POST",
            "/api/v1/sparklines/Gilgamesh",
            Some("{\n  \"item_ids\": [46010]\n}"),
        );
        assert!(post_curl.contains("-X POST 'https://ultros.app/api/v1/sparklines/Gilgamesh'"));
        assert!(post_curl.contains("-d '{\"item_ids\":[46010]}'"));
    }

    #[test]
    fn summarizes_union_and_nullable_openapi_31_schemas() {
        let union = json!({
            "oneOf": [
                { "$ref": "#/components/schemas/CheapestListings" },
                { "$ref": "#/components/schemas/CheapestListingsColumnar" }
            ]
        });
        assert_eq!(
            summarize_schema_type(&union),
            "CheapestListings | CheapestListingsColumnar"
        );

        let nullable = json!({
            "type": ["integer", "null"],
            "format": "int32"
        });
        assert_eq!(summarize_schema_type(&nullable), "integer (int32)?");

        let tuple = json!({
            "type": "array",
            "prefixItems": [
                { "$ref": "#/components/schemas/ActiveListing" },
                { "type": ["string", "null"] }
            ]
        });
        assert_eq!(summarize_schema_type(&tuple), "(ActiveListing, string?)");
    }

    #[test]
    fn truncates_utf8_cleanly_at_char_boundaries() {
        let text = "abc日本語def";
        let (prefix, truncated) = truncate_utf8_prefix(text, 5);
        assert!(truncated);
        assert_eq!(prefix, "abc");
    }
}
