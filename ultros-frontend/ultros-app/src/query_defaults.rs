pub use ultros_ui_query::query_defaults::*;

#[cfg(test)]
mod test {
    /// The invariant the fix rests on: no app code reaches the router's
    /// panicking URL hooks, so a new filter cannot quietly reintroduce #7305.
    /// The same trade `AppLink` made for `<A/>` — the wrapper is only worth
    /// anything while it is the *only* door.
    ///
    /// `query_defaults.rs` itself is the one file allowed to name them; the
    /// virtual-grid fixture is a dev harness mounted under the real shell.
    #[test]
    fn no_app_code_calls_the_panicking_router_url_hooks() {
        fn walk(dir: &std::path::Path, out: &mut Vec<(String, String)>) {
            let mut entries: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
                .expect("the crate's src tree is readable")
                .map(|e| e.expect("a readable directory entry").path())
                .collect();
            entries.sort();
            for path in entries {
                if path.is_dir() {
                    walk(&path, out);
                    continue;
                }
                if path.extension().is_none_or(|e| e != "rs") {
                    continue;
                }
                let src = std::fs::read_to_string(&path).expect("a readable source file");
                let full = path.to_string_lossy().replace('\\', "/");
                let name = match full.rsplit_once("/src/") {
                    Some((_, rel)) => rel.to_string(),
                    None => full,
                };
                // Only the production half: a test is allowed — required,
                // even — to call the panicking hook and prove it panics.
                let production = match src.split_once(&format!("#[cfg({})]", "test")) {
                    Some((head, rest))
                        if rest.trim_start().starts_with(&format!("mod {}", "test")) =>
                    {
                        head.to_string()
                    }
                    _ => src,
                };
                out.push((name, production));
            }
        }
        let mut files = Vec::new();
        walk(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut files,
        );
        // The invariant must follow components into their extracted crates.
        let frontend = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap();
        for entry in std::fs::read_dir(frontend).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy();
            if (name.starts_with("ultros-ui") || name == "ultros-frontend-core")
                && path.join("src").is_dir()
            {
                walk(&path.join("src"), &mut files);
            }
        }

        assert!(files.len() > 100, "the walk must reach the frontend crates");

        const ALLOWED: [&str; 2] = ["query_defaults.rs", "components/virtual_grid/fixture.rs"];
        let mut offenders = Vec::new();
        for (name, src) in &files {
            if ALLOWED.contains(&name.as_str()) {
                continue;
            }
            for hook in ["use_query_map", "query_signal_with_options"] {
                // Only real uses: a doc comment naming the hook is how the
                // fallbacks explain themselves.
                let used = src.lines().any(|line| {
                    !line.trim_start().starts_with("//")
                        && line
                            .match_indices(hook)
                            .any(|(at, _)| !line[at + hook.len()..].starts_with("_or_default"))
                });
                if used {
                    offenders.push(format!("{name} uses {hook}"));
                }
            }
            // `query_signal` cannot be matched by name — the app's own
            // wrapper shares it — so catch the router's copy by the path it
            // has to be reached through. Both a qualified call and a grouped
            // `use leptos_router::{hooks::{query_signal, ..}}` import are
            // whitespace-collapsed first, because rustfmt splits either one
            // across lines. #1316 arrived with exactly that import and nine
            // filters behind it, past a green version of this test.
            let flat = src
                .lines()
                .filter(|line| !line.trim_start().starts_with("//"))
                .collect::<Vec<_>>()
                .join(" ")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .replace(" ::", "::")
                .replace(":: ", "::")
                .replace("{ ", "{")
                .replace(" }", "}");
            if flat.contains("leptos_router::hooks::query_signal") {
                offenders.push(format!("{name} calls the router's query_signal"));
            }
            for statement in flat.split("use leptos_router").skip(1) {
                let statement = statement.split(';').next().unwrap_or_default();
                if statement.contains("query_signal") || statement.contains("use_query_map") {
                    offenders.push(format!("{name} imports a router URL hook: {statement}"));
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "route these through query_defaults instead: {offenders:?}"
        );
    }
}
