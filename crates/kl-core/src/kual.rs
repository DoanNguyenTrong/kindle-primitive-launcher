//! KUAL extensions: `menu.json` files under the extensions folder (`/mnt/us/extensions/<name>/menu.json`,
//! beside the documents folder). The launcher shows them behind one "Extensions" tile, as KUAL does: a list
//! with submenus, ordered by `priority` then name. An item with an `action` runs
//! `sh <extension>/<action> <params>` in the extension folder.
//!
//! Not used: `exitmenu`, `status`, `checked`, `model` and `if` checks. Those depend on KUAL's own runtime.

use crate::apps::{App, IconSource, Launch};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
};

/// One entry of an extension menu: a submenu (`children`) or a command.
#[derive(Debug, Clone, PartialEq)]
pub struct MenuItem {
    pub name: String,
    /// The shell command for an item that runs something.
    pub command: Option<String>,
    /// The extension folder: the command's working directory.
    pub dir: String,
    pub priority: i64,
    pub children: Vec<MenuItem>,
}

impl MenuItem {
    pub fn is_submenu(&self) -> bool {
        !self.children.is_empty()
    }

    /// The app the launcher hands to the supervisor to run this item.
    pub fn app(&self) -> Option<App> {
        let command = self.command.clone()?;
        Some(App {
            id: format!("kual:{}:{}", self.dir, self.name),
            name: self.name.clone(),
            icon: IconSource::None,
            launch: Launch::Shell { command },
            dir: self.dir.clone(),
            order: None,
            hidden: false,
        })
    }
}

/// The items of one `menu.json`. `ext_id` (the folder name) only labels problems.
pub fn parse(json: &str, ext_dir: &Path, ext_id: &str) -> Result<Vec<MenuItem>, String> {
    let value: Value = serde_json::from_str(json).map_err(|e| format!("{ext_id}/menu.json: {e}"))?;
    let list = value
        .get("items")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{ext_id}/menu.json: no items list"))?;
    Ok(items(list, ext_dir))
}

fn items(list: &[Value], dir: &Path) -> Vec<MenuItem> {
    let mut out = Vec::new();
    for item in list {
        let Some(name) = item.get("name").and_then(Value::as_str).filter(|n| !n.trim().is_empty()) else { continue };
        let priority = item.get("priority").and_then(Value::as_i64).unwrap_or(0);
        let children = item.get("items").and_then(Value::as_array).map(|c| items(c, dir)).unwrap_or_default();
        let command = if children.is_empty() {
            item.get("action").and_then(Value::as_str).filter(|a| !a.is_empty()).map(|action| {
                let mut command = format!("sh {}", shell_quote(&dir.join(action).to_string_lossy()));
                let params = item.get("params").and_then(Value::as_str).unwrap_or("").trim();
                if !params.is_empty() {
                    command.push(' ');
                    command.push_str(params);
                }
                command
            })
        } else {
            None
        };
        // An entry with neither a submenu nor a command does nothing: leave it out.
        if children.is_empty() && command.is_none() {
            continue;
        }
        out.push(MenuItem { name: name.trim().to_string(), command, dir: dir.to_string_lossy().into_owned(), priority, children });
    }
    sort(&mut out);
    out
}

fn sort(items: &mut [MenuItem]) {
    items.sort_by(|a, b| a.priority.cmp(&b.priority).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
}

/// Single-quote a path for `sh -c`.
fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// The top level of the Extensions list: the items of every extension under `root` with a `menu.json`, in
/// one list. Problems (unreadable or invalid files) are returned as text; the other extensions still load.
pub fn discover_root(root: &Path) -> (Vec<MenuItem>, Vec<String>) {
    let mut menu = Vec::new();
    let mut problems = Vec::new();
    let Ok(entries) = fs::read_dir(root) else { return (menu, problems) };
    let mut dirs: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    dirs.sort();
    for dir in dirs {
        let file = dir.join("menu.json");
        if !file.is_file() {
            continue;
        }
        let ext_id = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        match fs::read_to_string(&file) {
            Ok(text) => match parse(&text, &dir, &ext_id) {
                Ok(found) => menu.extend(found),
                Err(e) => problems.push(e),
            },
            Err(e) => problems.push(format!("{ext_id}/menu.json: {e}")),
        }
    }
    sort(&mut menu);
    (menu, problems)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MENU: &str = r#"{
        "items": [
            { "name": "Wi-Fi tools", "priority": 5, "items": [
                { "name": "Say hi", "action": "bin/hi.sh", "params": "--loud" },
                { "name": "Reset", "action": "bin/reset.sh", "priority": -1 }
            ]},
            { "name": "Just a menu", "items": [] },
            { "name": "Quoted", "action": "it's.sh", "priority": 1 },
            { "name": "", "action": "x.sh" },
            { "name": "No action" }
        ]
    }"#;

    #[test]
    fn submenus_keep_their_items_in_priority_order() {
        let menu = parse(MENU, Path::new("/mnt/us/extensions/tools"), "tools").unwrap();
        let names: Vec<_> = menu.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, ["Quoted", "Wi-Fi tools"]);
        let tools = &menu[1];
        assert!(tools.is_submenu());
        assert_eq!(tools.command, None);
        let inner: Vec<_> = tools.children.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(inner, ["Reset", "Say hi"]);
    }

    #[test]
    fn commands_quote_paths_and_pass_params() {
        let menu = parse(MENU, Path::new("/e"), "e").unwrap();
        assert_eq!(menu[0].command.as_deref(), Some("sh '/e/it'\\''s.sh'"));
        assert_eq!(menu[1].children[1].command.as_deref(), Some("sh '/e/bin/hi.sh' --loud"));
        let app = menu[1].children[0].app().unwrap();
        assert_eq!(app.launch, Launch::Shell { command: "sh '/e/bin/reset.sh'".into() });
        assert_eq!(app.dir, "/e");
        assert_eq!(menu[1].app(), None);
    }

    #[test]
    fn a_menu_without_items_is_a_problem() {
        let err = parse("{}", Path::new("/e"), "e").unwrap_err();
        assert!(err.contains("no items list"), "{err}");
        assert!(parse("{ not json", Path::new("/e"), "e").is_err());
    }

    #[test]
    fn discover_root_merges_extensions_and_reports_bad_ones() {
        let root = std::env::temp_dir().join(format!("kl-kual-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for d in ["good", "other", "bad", "no-menu"] {
            fs::create_dir_all(root.join(d)).unwrap();
        }
        fs::write(root.join("good/menu.json"), MENU).unwrap();
        fs::write(root.join("other/menu.json"), r#"{"items":[{"name":"Alpha","action":"a.sh","priority":1}]}"#).unwrap();
        fs::write(root.join("bad/menu.json"), "[]").unwrap();
        let (menu, problems) = discover_root(&root);
        let names: Vec<_> = menu.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, ["Alpha", "Quoted", "Wi-Fi tools"]);
        assert_eq!(problems.len(), 1);
        assert!(problems[0].starts_with("bad/menu.json"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_missing_root_is_not_a_problem() {
        let (menu, problems) = discover_root(Path::new("/definitely/not/here"));
        assert!(menu.is_empty() && problems.is_empty());
    }
}
