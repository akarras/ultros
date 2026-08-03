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
