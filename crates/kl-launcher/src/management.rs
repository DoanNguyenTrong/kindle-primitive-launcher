use std::path::Path;

use kl_core::{apps::STORE_ENTRY_FILE, App, Launch};

pub fn store_app(apps: &[App]) -> Option<App> {
    apps.iter()
        .find(|app| app.id == STORE_ENTRY_FILE || app.launch == Launch::Store)
        .cloned()
}

pub fn launch_descriptor(app: &App, documents: &Path, installed: bool) -> Option<String> {
    let mut descriptor = app.next_file()?;
    if installed {
        let Launch::Script { path, hooks: false } = &app.launch else {
            return None;
        };
        if Path::new(path) != documents.join(STORE_ENTRY_FILE) {
            return None;
        }
        descriptor.push_str("installed\n");
    }
    Some(descriptor)
}

/// The Store record can be read without a catalog or network connection.
pub fn installed_count(launcher: &Path) -> Option<usize> {
    let record = kl_store::record::load(&launcher.join("store/installed.json")).ok()?;
    Some(
        record
            .apps
            .iter()
            .filter(|(id, installed)| {
                installed.id == id.as_str()
                    && kl_store::catalog::valid_app_id(id)
                    && kl_store::Version::parse(&installed.version).is_some()
            })
            .count(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use kl_core::{discover, Config, IconSource};

    #[test]
    fn installed_destination_is_only_for_the_canonical_store_entry() {
        let documents = Path::new("/mnt/us/documents");
        let mut app = App {
            id: STORE_ENTRY_FILE.into(),
            name: "Store".into(),
            icon: IconSource::None,
            launch: Launch::Script {
                path: "/mnt/us/documents/Store.sh".into(),
                hooks: false,
            },
            dir: "/mnt/us/documents".into(),
            order: None,
            hidden: false,
            background: false,
        };
        assert_eq!(launch_descriptor(&app, documents, false), app.next_file());
        assert_eq!(
            launch_descriptor(&app, documents, true).as_deref(),
            Some("/mnt/us/documents\nscript\n/mnt/us/documents/Store.sh\ninstalled\n")
        );
        app.launch = Launch::Script {
            path: "/mnt/us/Store.sh".into(),
            hooks: false,
        };
        assert!(launch_descriptor(&app, documents, true).is_none());

        let (library, _) = discover(
            Path::new("/nonexistent-primitive-test-library"),
            &Config::default(),
        );
        assert_eq!(store_app(&library).unwrap().launch, Launch::Store);
    }

    #[test]
    fn counts_valid_store_receipts_with_no_catalog() {
        use kl_store::record::{self, Installed, Record};

        let launcher =
            std::env::temp_dir().join(format!("kl-launcher-managed-{}", std::process::id()));
        let path = launcher.join("store/installed.json");
        assert_eq!(installed_count(&launcher), Some(0));

        let mut record = Record::new();
        record.apps.insert(
            "valid-app".into(),
            Installed {
                id: "valid-app".into(),
                version: "0.1.0".into(),
                ..Installed::default()
            },
        );
        record.apps.insert(
            "missing-version".into(),
            Installed {
                id: "missing-version".into(),
                ..Installed::default()
            },
        );
        record.apps.insert(
            "mismatched-id".into(),
            Installed {
                id: "different-app".into(),
                version: "0.1.0".into(),
                ..Installed::default()
            },
        );
        record::save(&path, &record).unwrap();
        assert_eq!(installed_count(&launcher), Some(1));
        std::fs::write(&path, "invalid receipt").unwrap();
        assert_eq!(installed_count(&launcher), None);
        std::fs::remove_dir_all(launcher).unwrap();
    }
}
