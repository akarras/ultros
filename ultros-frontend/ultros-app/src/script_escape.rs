/// Escape a JSON string for safe embedding inside a `<script>` element.
///
/// JSON allows literal `<` characters inside strings; if any of them happen to
/// be followed by `/script>`, the parser would close the tag and start
/// executing arbitrary content as HTML. Replacing the handful of characters
/// below with their `\uXXXX` escapes keeps the payload as valid JSON and
/// inert to the HTML parser. U+2028 / U+2029 also need escaping because they
/// are JS line terminators (legal in JSON strings but break script parsing).
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
    fn test_escape_for_script_tag() {
        let json = r#"{"name": "test", "value": "<script>alert('xss')</script>&amp;\u2028\u2029"}"#;
        let escaped = escape_for_script_tag(json);
        assert_eq!(
            escaped,
            r#"{"name": "test", "value": "\u003cscript\u003ealert('xss')\u003c/script\u003e\u0026amp;\u2028\u2029"}"#
        );

        // verify no characters are dropped
        let s = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789!@#$^*()_-+={}[]|:;\"',.?/~`\\";
        assert_eq!(escape_for_script_tag(s), s);

        let c = "<\n>\n&\n\u{2028}\n\u{2029}";
        assert_eq!(
            escape_for_script_tag(c),
            "\\u003c\n\\u003e\n\\u0026\n\\u2028\n\\u2029"
        );
    }
}
