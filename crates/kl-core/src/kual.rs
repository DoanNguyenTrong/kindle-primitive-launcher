//! KUAL extensions: `menu.json` files under the extensions folder (`/mnt/us/extensions/<name>/menu.json`,
//! beside the documents folder). A subset of KUAL's format, enough for launching:
//!
//! - nested `items` are flattened into the grid as "Parent: Child";
//! - each item with an `action` becomes an app that runs `sh <extension>/<action> <params>` in the extension
//!   folder, ordered by `priority`.
//!
//! Not used: `exitmenu`, `status`, `checked`, `model` and `if` checks. Those need KUAL's own menu, which the
//! launcher does not have.

use crate::apps::{App, IconSource, Launch};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
};

/// The apps in one `menu.json`. `ext_id` is the extension's folder name, used in the app ids.
pub fn parse(json: &str, ext_dir: &Path, ext_id: &str) -> Result<Vec<App>, String> {
    let value: Value = serde_json::from_str(json).map_err(|e| format!("{ext_id}/menu.json: {e}"))?;
    let items = value
        .get("items")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{ext_id}/menu.json: no items list"))?;
    let mut apps = Vec::new();
    flatten(items, "", ext_dir, ext_id, &mut apps);
    Ok(apps)
}

fn flatten(items: &[Value], prefix: &str, dir: &Path, ext_id: &str, out: &mut Vec<App>) {
    for item in items {
        let Some(name) = item.get("name").and_then(Value::as_str).filter(|n| !n.is_empty()) else { continue };
        let full = if prefix.is_empty() { name.to_string() } else { format!("{prefix}: {name}") };
        if let Some(children) = item.get("items").and_then(Value::as_array) {
            flatten(children, &full, dir, ext_id, out);
        }
        let Some(action) = item.get("action").and_then(Value::as_str).filter(|a| !a.is_empty()) else { continue };
        let params = item.get("params").and_then(Value::as_str).unwrap_or("");
        let script = dir.join(action);
        let mut command = format!("sh {}", shell_quote(&script.to_string_lossy()));
        if !params.trim().is_empty() {
            command.push(' ');
            command.push_str(params.trim());
        }
        out.push(App {
            id: format!("kual:{ext_id}:{full}"),
            name: full,
            icon: IconSource::None,
            launch: Launch::Shell { command },
            dir: dir.to_string_lossy().into_owned(),
            order: item.get("priority").and_then(Value::as_i64),
            hidden: false,
        });
    }
}

/// Single-quote a path for `sh -c`.
fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// Every extension under `root` with a `menu.json`. Problems (unreadable or invalid files) are returned as
/// text for the notice; the other extensions still load.
pub fn discover_root(root: &Path) -> (Vec<App>, Vec<String>) {
    let mut apps = Vec::new();
    let mut problems = Vec::new();
    let Ok(entries) = fs::read_dir(root) else { return (apps, problems) };
    let mut dirs: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    dirs.sort();
    for dir in dirs {
        let menu = dir.join("menu.json");
        if !menu.is_file() {
            continue;
        }
        let ext_id = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        match fs::read_to_string(&menu) {
            Ok(text) => match parse(&text, &dir, &ext_id) {
                Ok(found) => apps.extend(found),
                Err(e) => problems.push(e),
            },
            Err(e) => problems.push(format!("{ext_id}/menu.json: {e}")),
        }
    }
    (apps, problems)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MENU: &str = r#"{
        "items": [
            { "name": "Wi-Fi tools", "items": [
                { "name": "Reset", "action": "bin/reset.sh", "priority": 2 },
                { "name": "Say hi", "action": "bin/hi.sh", "params": "--loud" }
            ]},
            { "name": "Just a menu", "items": [] },
            { "name": "Nameless", "action": "x.sh" },
            { "name": "Quoted", "action": "it's.sh", "priority": 1 }
        ]
    }"#;

    #[test]
    fn nested_items_are_flattened_with_their_actions() {
        let apps = parse(MENU, Path::new("/mnt/us/extensions/tools"), "tools").unwrap();
        let names: Vec<_> = apps.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["Wi-Fi tools: Reset", "Wi-Fi tools: Say hi", "Nameless", "Quoted"]);
        assert_eq!(apps[0].id, "kual:tools:Wi-Fi tools: Reset");
        assert_eq!(apps[0].dir, "/mnt/us/extensions/tools");
        assert_eq!(apps[0].order, Some(2));
        assert_eq!(
            apps[0].launch,
            Launch::Shell { command: "sh '/mnt/us/extensions/tools/bin/reset.sh'".into() }
        );
    }

    #[test]
    fn params_are_passed_and_quotes_in_paths_survive() {
        let apps = parse(MENU, Path::new("/e"), "e").unwrap();
        assert_eq!(apps[1].launch, Launch::Shell { command: "sh '/e/bin/hi.sh' --loud".into() });
        assert_eq!(apps[3].launch, Launch::Shell { command: "sh '/e/it'\\''s.sh'".into() });
    }

    #[test]
    fn a_menu_without_items_is_a_problem() {
        let err = parse("{}", Path::new("/e"), "e").unwrap_err();
        assert!(err.contains("no items list"), "{err}");
        assert!(parse("{ not json", Path::new("/e"), "e").is_err());
    }

    #[test]
    fn discover_root_loads_good_extensions_and_reports_bad_ones() {
        let root = std::env::temp_dir().join(format!("kl-kual-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("good")).unwrap();
        fs::create_dir_all(root.join("bad")).unwrap();
        fs::create_dir_all(root.join("no-menu")).unwrap();
        fs::write(root.join("good/menu.json"), MENU).unwrap();
        fs::write(root.join("bad/menu.json"), "[]").unwrap();
        let (apps, problems) = discover_root(&root);
        assert_eq!(apps.len(), 4);
        assert!(apps.iter().all(|a| a.id.starts_with("kual:good:")));
        assert_eq!(problems.len(), 1);
        assert!(problems[0].starts_with("bad/menu.json"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_missing_root_is_not_a_problem() {
        let (apps, problems) = discover_root(Path::new("/definitely/not/here"));
        assert!(apps.is_empty() && problems.is_empty());
    }
}
