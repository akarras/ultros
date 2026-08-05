//! Diagnostic: for a range of icon ids, classify each as readable / present-but-
//! unreadable / absent, and report the texture format. Distinguishes "the client
//! doesn't have this icon" from "ironworks can't parse this icon".
//!
//! cargo run --release -p icon-extract --example probe -- [start] [end] [step]

use ironworks::file::tex::Texture;
use std::collections::BTreeMap;

fn icon_path(icon_id: i32, hr: bool) -> String {
    let group = icon_id / 1000 * 1000;
    let suffix = if hr { "_hr1" } else { "" };
    format!("ui/icon/{group:06}/{icon_id:06}{suffix}.tex")
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let start: i32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(1);
    let end: i32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(250_000);
    let step: i32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(1);

    let root = std::env::var("FFXIV_PATH").ok();
    let install = icon_extract::GameInstall::discover(root.as_deref().map(std::path::Path::new))
        .expect("no FFXIV install found; set FFXIV_PATH");
    println!("install version: {}", install.version);
    let ironworks = install.ironworks();

    let mut readable = 0u32;
    let mut absent = 0u32;
    let mut formats: BTreeMap<String, u32> = BTreeMap::new();
    // icon id -> error text, for entries that exist but fail to parse
    let mut unreadable: Vec<(i32, bool, String)> = Vec::new();
    // highest icon id that read cleanly, to see where coverage stops
    let mut max_ok = 0i32;
    let mut readable_ids: Vec<i32> = Vec::new();

    let mut id = start;
    while id <= end {
        let mut got = false;
        let mut err_seen: Option<(bool, String)> = None;
        for hr in [true, false] {
            match ironworks.file::<Texture>(&icon_path(id, hr)) {
                Ok(tex) => {
                    *formats.entry(format!("{:?}", tex.format())).or_default() += 1;
                    got = true;
                    readable_ids.push(id);
                    max_ok = max_ok.max(id);
                    break;
                }
                Err(e) => {
                    let s = e.to_string();
                    // "not found" is a genuine absence; anything else is a
                    // parse/read failure on an entry that IS indexed.
                    let missing = s.to_lowercase().contains("could not be found")
                        || s.to_lowercase().contains("not found");
                    if !missing && err_seen.is_none() {
                        err_seen = Some((hr, s));
                    }
                }
            }
        }
        if got {
            readable += 1;
        } else if let Some((hr, s)) = err_seen {
            unreadable.push((id, hr, s));
        } else {
            absent += 1;
        }
        id += step;
    }

    println!("scanned {start}..={end} step {step}");
    println!("  readable            : {readable}  (highest ok id: {max_ok})");
    println!("  absent (not indexed): {absent}");
    println!("  PRESENT-BUT-UNREADABLE: {}", unreadable.len());
    println!("  formats seen: {formats:?}");
    // Where do the unparseable entries live? Compare against readable ones in
    // the same bucket — a uniform ratio means this is not a "newest content"
    // problem, a rising one means it is.
    let mut ok_by: BTreeMap<i32, u32> = BTreeMap::new();
    let mut bad_by: BTreeMap<i32, u32> = BTreeMap::new();
    for (id, _, _) in &unreadable {
        *bad_by.entry(id / 20000).or_default() += 1;
    }
    for id in &readable_ids {
        *ok_by.entry(id / 20000).or_default() += 1;
    }
    println!("  bucket(20k)  readable  unreadable  bad%");
    for b in 0..=(end / 20000) {
        let ok = *ok_by.get(&b).unwrap_or(&0);
        let bad = *bad_by.get(&b).unwrap_or(&0);
        if ok + bad == 0 {
            continue;
        }
        println!(
            "    {:>6}      {:>5}     {:>5}     {:>3}%",
            b * 20000,
            ok,
            bad,
            bad * 100 / (ok + bad)
        );
    }
}
