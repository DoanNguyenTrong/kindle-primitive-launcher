//! Starts the standalone Store, installing its verified package if it is absent.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use std::sync::mpsc::Sender;
use std::thread;

use kl_core::{App, Launch};
pub use kl_core::apps::STORE_ENTRY_FILE as ENTRY;
use kl_kindle::{device, log, Ev};
use kl_store::{catalog::Listing, install::Paths, pipeline, resolve};

const CATALOG: &str = "https://doannguyentrong.github.io/kindle-primitive-catalog/v1";
const ID: &str = kl_core::store::STORE_ID;
const DIR: &str = "store";
const BINARY: &str = "store/bin/kl-store-app";

pub fn store_present(documents: &Path) -> bool {
    documents.join(ENTRY).is_file()
        && documents.parent().is_some_and(|root| {
            fs::metadata(root.join(BINARY))
                .is_ok_and(|file| file.is_file() && file.permissions().mode() & 0o111 != 0)
        })
}

pub fn reconcile_tiles(apps: &mut Vec<App>, documents: &Path) {
    // An incomplete manual copy should offer repair from the built-in tile.
    let present = store_present(documents);
    apps.retain(|app| {
        if app.launch == Launch::Store {
            !present
        } else if app.id == ENTRY {
            present
        } else {
            true
        }
    });
}

pub fn available_updates(launcher: &Path) -> usize {
    let Some(device) = device::store_device() else { return 0 };
    let Ok(record) = kl_store::record::load(&launcher.join("store/installed.json")) else { return 0 };
    record.apps.iter().filter(|(id, installed)| {
        if !kl_store::catalog::valid_app_id(id) {
            return false;
        }
        let Ok(text) = fs::read_to_string(launcher.join("store/cache/apps").join(format!("{id}.json"))) else { return false };
        let Ok(listing) = serde_json::from_str::<Listing>(&text) else { return false };
        if listing.id != **id {
            return false;
        }
        let Ok(version) = resolve::pick_version(&listing, &device) else { return false };
        let current = kl_store::Version::parse(&installed.version);
        current.is_some() && kl_store::Version::parse(&version.version) > current
    }).count()
}

pub fn entry_for(mut app: App, documents: &Path) -> Option<App> {
    if !store_present(documents) {
        return None;
    }
    app.name = "Primitive Store".into();
    app.dir = documents.to_string_lossy().into_owned();
    app.launch = Launch::Script {
        path: documents.join(ENTRY).to_string_lossy().into_owned(),
        hooks: false,
    };
    Some(app)
}

pub fn start(tx: Sender<Ev>, documents: std::path::PathBuf) {
    thread::spawn(move || {
        let report = |message: &str| {
            log(message);
            let _ = tx.send(Ev::Notice(message.into()));
        };
        let result = std::panic::catch_unwind(|| install(&documents, report))
            .unwrap_or_else(|_| Err("An internal error occurred.".into()));
        let _ = tx.send(Ev::StoreBootstrapped(result));
    });
}

fn install(documents: &Path, report: impl Fn(&str)) -> Result<(), String> {
    let root = documents.parent().ok_or("Kindle storage is unavailable.")?;
    let device =
        device::store_device().ok_or("The Kindle's firmware version could not be read.")?;
    report("Finding Primitive Store…");
    let policy = primitive_net::Policy::store(kl_store::refresh::MAX_LISTING);
    let text =
        primitive_net::fetch_text(&format!("{CATALOG}/apps/{ID}.json"), &policy).map_err(|_| {
            "The Store package could not be found in the catalog. Check Wi-Fi and try again."
                .to_string()
        })?;
    let listing = parse_listing(&text)?;
    let version = resolve::pick_version(&listing, &device).map_err(|e| e.to_string())?;
    let asset = &version.assets[&device.arch];
    let available = device::available_storage_bytes(root)
        .map_err(|_| "Available Kindle storage could not be checked.")?;
    if available / 3 < asset.size {
        return Err("Not enough space. Free some Kindle storage and try again.".into());
    }

    let paths = Paths::under(root);
    let downloads = root.join("launcher/store/downloads");
    fs::create_dir_all(&downloads).map_err(|e| e.to_string())?;
    let zip = downloads.join("primitive-store-bootstrap.zip");
    report("Downloading Primitive Store…");
    primitive_net::download_resumable(
        &asset.url,
        &zip,
        &primitive_net::Policy::store(asset.size),
        asset.size,
        &asset.sha256,
        |_, _| {},
    )
    .map_err(|e| format!("Download failed. Check Wi-Fi and try again. {e}"))?;
    report("Checking and installing Primitive Store…");
    let date = Command::new("date")
        .arg("+%Y-%m-%d")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_default();
    install_package(&paths, &listing, &device, &zip, &date)?;
    if !store_present(documents) {
        return Err("The package did not provide a working Store entry.".into());
    }
    Ok(())
}

fn parse_listing(text: &str) -> Result<Listing, String> {
    let listing: Listing =
        serde_json::from_str(text).map_err(|_| "The Store listing could not be read.")?;
    if listing.schema != 1
        || listing.id != ID
        || listing.dir() != DIR
        || listing.source != kl_store::catalog::Source::Hosted
    {
        return Err("The catalog did not return the expected Store listing.".into());
    }
    Ok(listing)
}

fn install_package(
    paths: &Paths,
    listing: &Listing,
    device: &resolve::Device,
    zip: &Path,
    date: &str,
) -> Result<String, String> {
    let result = (|| {
        let version = resolve::pick_version(listing, device).map_err(|e| e.to_string())?;
        let asset = &version.assets[&device.arch];
        // Check the fingerprint before opening the archive. A usable bootstrap
        // must install both files in the same transaction.
        kl_store::verify::verify_file(zip, &asset.sha256, asset.size).map_err(|e| e.to_string())?;
        let manifest = kl_store::unpack::read_manifest(zip).map_err(|e| e.to_string())?;
        if manifest.dir != DIR
            || ![BINARY, "documents/Store.sh"]
                .iter()
                .all(|required| manifest.files.iter().any(|file| file == required))
        {
            return Err("The Store package is missing its program or library entry.".into());
        }
        pipeline::install_downloaded(paths, listing, device, zip, &asset.url, date)
    })();
    let _ = fs::remove_file(zip);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("kl-bootstrap-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(path.join("documents")).unwrap();
        path
    }

    #[test]
    fn a_complete_copy_has_one_store_tile_and_can_be_launched_by_the_supervisor() {
        let root = fixture("tiles");
        let documents = root.join("documents");
        fs::write(
            documents.join(ENTRY),
            "#!/bin/sh\n# Name: Primitive Store\n",
        )
        .unwrap();
        let discover = || kl_core::discover(&documents, &kl_core::Config::default()).0;
        let mut apps = discover();
        reconcile_tiles(&mut apps, &documents);
        let built_in = apps
            .iter()
            .find(|app| app.launch == Launch::Store)
            .unwrap()
            .clone();
        assert!(apps.iter().all(|app| app.id != ENTRY));
        assert!(entry_for(built_in.clone(), &documents).is_none());

        fs::create_dir_all(root.join(DIR).join("bin")).unwrap();
        fs::write(root.join(BINARY), "program").unwrap();
        assert!(!store_present(&documents));
        fs::set_permissions(root.join(BINARY), fs::Permissions::from_mode(0o755)).unwrap();
        let mut apps = discover();
        reconcile_tiles(&mut apps, &documents);
        assert!(apps.iter().all(|app| app.launch != Launch::Store));
        assert!(apps.iter().any(|app| app.id == ENTRY));
        let entry = entry_for(built_in, &documents)
            .unwrap()
            .next_file()
            .unwrap();
        assert_eq!(
            entry,
            format!(
                "{}\nscript\n{}\n",
                documents.display(),
                documents.join(ENTRY).display()
            )
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn bootstrap_installs_the_staged_store_archive() {
        // Also exercised with the real ARM package by the host simulator.
        let Some(package) = std::env::var_os("KL_STORE_BOOTSTRAP_PACKAGE") else {
            return;
        };
        let root = fixture("package");
        let zip = root.join("store.zip");
        fs::copy(package, &zip).unwrap();
        let manifest = kl_store::unpack::read_manifest(&zip).unwrap();
        let (sha256, size) = kl_store::verify::sha256_file(&zip).unwrap();
        let text = serde_json::json!({
            "schema": 1, "id": ID, "dir": DIR, "name": "Primitive Store",
            "subtitle": "Apps for your Kindle", "developer": {"name": "Primitive AI"},
            "categories": ["utilities"], "description": "Browse Kindle apps.",
            "repo": "https://github.com/DoanNguyenTrong/kindle-primitive-store",
            "licence": "PolyForm-Noncommercial-1.0.0", "source": "hosted", "icon": "icon.png",
            "changes": {"network": true, "stops_kindle_ui": true, "starts_at_boot": false, "root_filesystem": false, "hooks": false},
            "versions": [{
                "version": manifest.version, "date": "2026-11-19", "notes": "Local package verification",
                "firmware": manifest.firmware, "requires": manifest.requires,
                "assets": {(manifest.arch.clone()): {"url": "https://github.com/example/releases/store.zip", "sha256": sha256, "size": size}}
            }]
        }).to_string();
        let listing = parse_listing(&text).unwrap();
        let device = resolve::Device {
            arch: manifest.arch,
            firmware: kl_store::Version::parse("5.18.2").unwrap(),
        };
        let paths = Paths::under(&root);
        let installed = install_package(&paths, &listing, &device, &zip, "2026-11-19").unwrap();
        assert_eq!(installed, manifest.version);
        assert!(!zip.exists());
        assert!(store_present(&root.join("documents")));
        assert!(kl_store::record::load(&paths.record)
            .unwrap()
            .apps
            .contains_key(ID));
        fs::remove_dir_all(root).unwrap();
    }
}
