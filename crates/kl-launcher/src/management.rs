use std::{
    io,
    path::{Path, PathBuf},
    sync::mpsc::Sender,
    thread,
};

use kl_core::{
    storage_lease::StorageLease,
    store::{LAUNCHER_ID, STORE_ID},
    store_open,
    ui::InstalledApp,
    App,
};
use kl_kindle::{package_hooks::KindleHookRunner, Ev};
use kl_store::{
    catalog::valid_app_id,
    install::{self, Paths},
    record, Version,
};

fn removable(id: &str) -> bool {
    valid_app_id(id) && !matches!(id, LAUNCHER_ID | STORE_ID)
}

/// Installed packages come from the receipt. Scriptlets supply display names, including hidden apps.
pub fn installed_apps(launcher: &Path, apps: &[App]) -> Option<Vec<InstalledApp>> {
    let receipt = record::load(&launcher.join("store/installed.json")).ok()?;
    let mut rows: Vec<_> = receipt
        .apps
        .into_iter()
        .filter_map(|(id, installed)| {
            if installed.id != id
                || !valid_app_id(&id)
                || Version::parse(&installed.version).is_none()
            {
                return None;
            }
            let name = match id.as_str() {
                LAUNCHER_ID => "Primitive Launcher".into(),
                STORE_ID => "Store".into(),
                _ => store_open::entry_file(&installed).map_or_else(
                    || id.clone(),
                    |file| {
                        apps.iter().find(|app| app.id == file).map_or_else(
                            || file.trim_end_matches(".sh").to_owned(),
                            |app| app.name.clone(),
                        )
                    },
                ),
            };
            Some(InstalledApp {
                can_remove: removable(&id),
                id,
                name,
                version: installed.version,
            })
        })
        .collect();
    rows.sort_by_cached_key(|app| (app.name.to_lowercase(), app.id.clone()));
    Some(rows)
}

pub fn start(tx: Sender<Ev>, root: PathBuf, mut lease: StorageLease, app: Option<InstalledApp>) {
    thread::spawn(move || {
        let paths = Paths::under(&root);
        let acquired = lease.begin_write();
        let storage_held = acquired.is_ok();
        let result = match acquired {
            Ok(()) => std::panic::catch_unwind(|| change_installed_apps(&paths, app.as_ref()))
                .unwrap_or_else(|_| Err("An internal error occurred.".into())),
            Err(error) => Err(error.to_string()),
        };
        let message = match (result, app) {
            (Ok(()), Some(app)) => Some(format!("Removed {}.", app.name)),
            (Ok(()), None) => None,
            (Err(error), Some(app)) => Some(format!("Could not remove {}. {error}", app.name)),
            (Err(error), None) => Some(format!(
                "Could not recover the last application change. {error}"
            )),
        };
        let _ = tx.send(Ev::ManagedAppsChanged {
            lease,
            storage_held,
            message,
        });
    });
}

fn change_installed_apps(paths: &Paths, app: Option<&InstalledApp>) -> Result<(), String> {
    if let Some(app) = app {
        if !removable(&app.id) {
            return Err("This system app cannot be removed.".into());
        }
    }
    install::recover_with_hooks(paths, &KindleHookRunner).map_err(|error| error.to_string())?;
    if let Some(app) = app {
        remove_installed(paths, app).map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn remove_installed(paths: &Paths, app: &InstalledApp) -> Result<(), install::InstallError> {
    let receipt = record::load(&paths.record)?;
    if receipt
        .apps
        .get(&app.id)
        .is_some_and(|installed| installed.version != app.version)
    {
        return Err(
            io::Error::other("The app version changed. Review the list and try again.").into(),
        );
    }
    install::remove_with_hooks(paths, &app.id, &KindleHookRunner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kl_core::{discover, Config};
    use kl_store::record::{Installed, Record};
    use std::fs;

    #[test]
    fn removes_receipted_files_without_a_catalog_and_preserves_app_data() {
        let root = std::env::temp_dir().join(format!("kl-launcher-remove-{}", std::process::id()));
        let documents = root.join("documents");
        let paths = Paths::under(&root);
        fs::create_dir_all(&documents).unwrap();
        fs::create_dir_all(root.join("fixture/data")).unwrap();
        fs::write(
            documents.join("Fixture.sh"),
            "#!/bin/sh\n# Name: Offline fixture\n",
        )
        .unwrap();
        fs::write(root.join("fixture/binary"), b"app").unwrap();
        fs::write(root.join("fixture/data/note.txt"), b"personal data").unwrap();
        let mut record = Record::new();
        record.apps.insert(
            "fixture".into(),
            Installed {
                id: "fixture".into(),
                dir: "fixture".into(),
                version: "1.0.0".into(),
                files: vec!["documents/Fixture.sh".into(), "fixture/binary".into()],
                ..Installed::default()
            },
        );
        for id in [LAUNCHER_ID, STORE_ID] {
            record.apps.insert(
                id.into(),
                Installed {
                    id: id.into(),
                    version: "1.0.0".into(),
                    ..Installed::default()
                },
            );
        }
        record
            .apps
            .insert("bad-record".into(), Installed::default());
        record::save(&paths.record, &record).unwrap();
        let (apps, _) = discover(&documents, &Config::default());
        let rows = installed_apps(&root.join("launcher"), &apps).unwrap();
        assert_eq!(rows.len(), 3);
        let app = rows.iter().find(|row| row.id == "fixture").unwrap();
        assert_eq!(app.name, "Offline fixture");
        let blocked = rows.iter().find(|row| row.id == STORE_ID).unwrap();
        assert!(!blocked.can_remove);
        assert!(change_installed_apps(&paths, Some(blocked)).is_err());
        assert!(change_installed_apps(
            &paths,
            Some(&InstalledApp {
                version: "0.9.0".into(),
                ..app.clone()
            })
        )
        .is_err());
        change_installed_apps(&paths, Some(app)).unwrap();
        assert!(!documents.join("Fixture.sh").exists());
        assert!(!root.join("fixture/binary").exists());
        assert_eq!(
            fs::read(root.join("fixture/data/note.txt")).unwrap(),
            b"personal data"
        );
        let remaining = record::load(&paths.record).unwrap();
        assert!(!remaining.apps.contains_key("fixture"));
        assert!(remaining.apps.contains_key(STORE_ID));
        assert!(remaining.apps.contains_key(LAUNCHER_ID));
        fs::remove_dir_all(root).unwrap();
    }
}
