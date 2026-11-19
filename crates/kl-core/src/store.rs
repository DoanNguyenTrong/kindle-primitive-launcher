//! The store's entries as the screens show them: one per listing in the catalog cache, with what is installed and
//! what the store would do for it. Built from `kl-store`'s listing and the installed record; no device code here.

use kl_store::catalog::{Changes, Listing, Source};
use kl_store::record::Installed;
use kl_store::Version;

/// Whether an entry is installed, and whether a newer version is ready.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    NotInstalled,
    Installed(String),
    Update { installed: String, latest: String },
}

/// What the action button does: install from the store (hosted or upstream), or show the page to read (link).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Install,
    Link,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreEntry {
    pub id: String,
    pub name: String,
    pub subtitle: String,
    pub developer: String,
    pub description: String,
    pub categories: Vec<String>,
    pub licence: String,
    pub repo: String,
    pub kind: Kind,
    pub latest: Option<String>,
    pub status: Status,
    /// What the app does outside its own folder, in the words the install sheet uses.
    pub changes: Vec<String>,
}

/// The newest version that is not withdrawn, by version order.
pub fn newest_live(listing: &Listing) -> Option<String> {
    listing
        .versions
        .iter()
        .filter(|v| v.withdrawn.is_none())
        .filter_map(|v| Version::parse(&v.version).map(|p| (p, v.version.clone())))
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, s)| s)
}

/// The "What this app changes" lines, from the listing's flags (store-install-apps §4).
pub fn change_lines(c: &Changes) -> Vec<String> {
    let mut lines = Vec::new();
    if c.network {
        lines.push("Uses the internet.".to_string());
    }
    if c.stops_kindle_ui {
        lines.push("Stops the Kindle's own screen while it runs.".to_string());
    }
    if c.starts_at_boot {
        lines.push("Starts when the Kindle starts.".to_string());
    }
    if c.root_filesystem {
        lines.push("Changes the Kindle's system files.".to_string());
    }
    if c.hooks {
        lines.push("Runs its own install steps.".to_string());
    }
    if lines.is_empty() {
        lines.push("Changes nothing outside its own folder.".to_string());
    }
    if !c.notes.is_empty() {
        lines.push(c.notes.clone());
    }
    lines
}

impl StoreEntry {
    /// The entry for a listing, given what is installed under its id.
    pub fn from_listing(listing: &Listing, installed: Option<&Installed>) -> StoreEntry {
        let latest = newest_live(listing);
        let status = match (installed, &latest) {
            (None, _) => Status::NotInstalled,
            (Some(i), Some(l)) if Version::parse(l) > Version::parse(&i.version) => {
                Status::Update { installed: i.version.clone(), latest: l.clone() }
            }
            (Some(i), _) => Status::Installed(i.version.clone()),
        };
        StoreEntry {
            id: listing.id.clone(),
            name: listing.name.clone(),
            subtitle: listing.subtitle.clone(),
            developer: listing.developer.name.clone(),
            description: listing.description.clone(),
            categories: listing.categories.clone(),
            licence: listing.licence.clone(),
            repo: listing.repo.clone(),
            kind: if listing.source == Source::Link { Kind::Link } else { Kind::Install },
            latest,
            status,
            changes: change_lines(&listing.changes),
        }
    }

    /// The status line on the app's page, in the store's words.
    pub fn status_line(&self) -> String {
        if self.kind == Kind::Link {
            return "Listed by link: the store does not install it.".to_string();
        }
        match (&self.status, &self.latest) {
            (Status::NotInstalled, Some(v)) => format!("Version {v}"),
            (Status::NotInstalled, None) => "No version for this Kindle".to_string(),
            (Status::Installed(v), _) => format!("Installed: version {v}"),
            (Status::Update { installed, latest }, _) => format!("Update ready: {installed} to {latest}"),
        }
    }

    /// The action button's word, for the row and the page.
    pub fn action_label(&self) -> &'static str {
        match (self.kind, &self.status) {
            (Kind::Link, _) => "View",
            (Kind::Install, Status::NotInstalled) => "Get",
            (Kind::Install, Status::Update { .. }) => "Update",
            (Kind::Install, Status::Installed(_)) => "Installed",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample_listing() -> Listing {
        serde_json::from_str(
            r#"{
            "schema": 1, "id": "primitive-browser", "dir": "browser",
            "name": "Primitive Browser", "subtitle": "A browser for e-ink",
            "developer": {"name": "Primitive AI"}, "categories": ["internet"],
            "description": "A web browser made for e-ink, with night mode and image switch.",
            "repo": "https://github.com/DoanNguyenTrong/kindle-primitive-browser", "licence": "PolyForm-Noncommercial-1.0.0",
            "source": "hosted", "icon": "icon.png",
            "changes": {"network": true, "stops_kindle_ui": false, "starts_at_boot": false, "root_filesystem": false, "hooks": false},
            "versions": [
              {"version": "0.2.18", "date": "2026-11-20", "notes": "Newest.", "firmware": {"min": "5.16.3"},
               "tested_on": ["KindleOasis3"], "assets": {"armhf": {"url": "https://example.com/a.zip", "sha256": "aa", "size": 10}}},
              {"version": "0.2.17", "date": "2026-11-19", "notes": "Older.", "firmware": {"min": "5.16.3"},
               "tested_on": ["KindleOasis3"], "assets": {"armhf": {"url": "https://example.com/b.zip", "sha256": "bb", "size": 10}}}
            ]
        }"#,
        )
        .expect("the sample listing is valid")
    }

    fn installed(version: &str) -> Installed {
        Installed {
            id: "primitive-browser".into(),
            dir: "browser".into(),
            version: version.into(),
            arch: "armhf".into(),
            installed_at: "2026-11-19".into(),
            source_url: String::new(),
            sha256: String::new(),
            files: vec![],
            requires: Default::default(),
        }
    }

    #[test]
    fn a_missing_install_is_get_and_the_newest_live_version_is_shown() {
        let e = StoreEntry::from_listing(&sample_listing(), None);
        assert_eq!(e.status, Status::NotInstalled);
        assert_eq!(e.latest.as_deref(), Some("0.2.18"));
        assert_eq!(e.action_label(), "Get");
        assert_eq!(e.changes, vec!["Uses the internet."]);
    }

    #[test]
    fn an_older_install_offers_an_update_and_the_same_version_is_installed() {
        let l = sample_listing();
        let old = StoreEntry::from_listing(&l, Some(&installed("0.2.17")));
        assert_eq!(old.status, Status::Update { installed: "0.2.17".into(), latest: "0.2.18".into() });
        assert_eq!(old.action_label(), "Update");
        let same = StoreEntry::from_listing(&l, Some(&installed("0.2.18")));
        assert_eq!(same.status, Status::Installed("0.2.18".into()));
        assert_eq!(same.action_label(), "Installed");
    }

    #[test]
    fn a_withdrawn_version_is_not_offered_and_a_link_listing_says_view() {
        let mut l = sample_listing();
        l.versions[0].withdrawn = Some("broken".into());
        assert_eq!(newest_live(&l).as_deref(), Some("0.2.17"));
        l.source = Source::Link;
        let e = StoreEntry::from_listing(&l, None);
        assert_eq!(e.kind, Kind::Link);
        assert_eq!(e.action_label(), "View");
    }

    #[test]
    fn the_status_line_says_what_is_installed_and_what_is_ready() {
        let l = sample_listing();
        assert_eq!(StoreEntry::from_listing(&l, None).status_line(), "Version 0.2.18");
        assert_eq!(
            StoreEntry::from_listing(&l, Some(&installed("0.2.17"))).status_line(),
            "Update ready: 0.2.17 to 0.2.18"
        );
        assert_eq!(StoreEntry::from_listing(&l, Some(&installed("0.2.18"))).status_line(), "Installed: version 0.2.18");
        let mut link = l.clone();
        link.source = Source::Link;
        assert!(StoreEntry::from_listing(&link, None).status_line().starts_with("Listed by link"));
    }

    #[test]
    fn a_change_with_no_flags_says_it_changes_nothing_outside_its_folder() {
        let c = Changes { network: false, stops_kindle_ui: false, starts_at_boot: false, root_filesystem: false, hooks: false, notes: String::new() };
        assert_eq!(change_lines(&c), vec!["Changes nothing outside its own folder."]);
    }
}
