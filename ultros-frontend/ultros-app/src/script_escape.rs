//! Escaping for JSON payloads embedded in inline `<script>` elements.

/// Escape a JSON string for safe embedding inside a `<script>` element.
///
/// JSON allows literal `<` characters inside strings; if any of them happen to
/// be followed by `/script>`, the parser would close the tag and start
/// executing arbitrary content as HTML. Replacing the handful of characters
/// below with their `\uXXXX` escapes keeps the payload as valid JSON and
/// inert to the HTML parser. U+2028 / U+2029 also need escaping because they
/// are JS line terminators (legal in JSON strings but break script parsing).
///
/// Applying this to a whole serialized document is safe: outside of string
/// literals JSON only uses `{}[]",:` plus numbers and bare keywords, so none
/// of the replaced characters can occur there.
pub fn escape_for_script_tag(json: &str) -> String {
    let mut out = String::with_capacity(json.len());
    for c in json.chars() {
        match c {
            '<' => out.push_str("\\u003c"),
            '>' => out.push_str("\\u003e"),
            '&' => out.push_str("\\u0026"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closing_script_tag_cannot_survive() {
        let payload = serde_json::to_string(&serde_json::json!({
            "item": "</script><script>alert(1)</script>"
        }))
        .unwrap();
        // serde_json alone leaves `<` and `>` literal, which is the bug.
        assert!(payload.contains("</script>"));

        // What actually terminates a script element is the literal `</`
        // sequence. `/script>` may well survive escaping — harmless, because
        // the `<` that would arm it no longer exists anywhere in the output.
        let escaped = escape_for_script_tag(&payload);
        assert!(!escaped.contains('<'), "escaped output still has `<`");
        assert!(!escaped.contains('>'), "escaped output still has `>`");
        assert!(
            !escaped.contains("</"),
            "escaped output can still close a tag"
        );
    }

    #[test]
    fn escaping_round_trips_back_to_the_original_value() {
        // The escapes must stay valid JSON, or Google silently drops the
        // structured data instead of us noticing.
        let original = "</script>&\u{2028}\u{2029}<b>";
        let payload = serde_json::to_string(&serde_json::json!({ "v": original })).unwrap();
        let escaped = escape_for_script_tag(&payload);

        let parsed: serde_json::Value = serde_json::from_str(&escaped).unwrap();
        assert_eq!(parsed["v"], original);
    }

    #[test]
    fn ampersand_and_js_line_terminators_are_escaped() {
        assert_eq!(escape_for_script_tag("a&b"), "a\\u0026b");
        assert_eq!(escape_for_script_tag("a\u{2028}b"), "a\\u2028b");
        assert_eq!(escape_for_script_tag("a\u{2029}b"), "a\\u2029b");
    }

    #[test]
    fn ordinary_json_is_left_alone() {
        let payload = r#"{"name":"Iron Ingot","position":3}"#;
        assert_eq!(escape_for_script_tag(payload), payload);
    }
}
