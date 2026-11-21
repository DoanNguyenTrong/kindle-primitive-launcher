//! Starts the standalone Store, installing its verified package if it is absent.

use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::mpsc::Sender;
use std::thread;

use kl_core::apps::{store_present, STORE_BINARY as BINARY};
pub use kl_core::apps::{store_entry as entry_for, STORE_ENTRY_FILE as ENTRY};
use kl_kindle::{device, log, Ev};
use kl_store::catalog::{Asset, Listing};
use kl_store::signatures::TrustedKey;
use kl_store::{install::Paths, pipeline, refresh, resolve};

const CATALOG: &str = "https://primitiveos.github.io/kindle-primitive-catalog/v1";
const ID: &str = kl_core::store::STORE_ID;
const DIR: &str = "store";

struct NetSource;

impl refresh::Source for NetSource {
    fn text(&self, url: &str) -> Result<String, String> {
        primitive_net::fetch_text(url, &primitive_net::Policy::store(refresh::MAX_INDEX)).map_err(|error| error.to_string())
    }

    fn bytes(&self, url: &str, max: u64) -> Result<Vec<u8>, String> {
        primitive_net::fetch_bytes(url, &primitive_net::Policy::store(max)).map_err(|error| error.to_string())
    }
}

pub fn available_updates(launcher: &Path) -> usize {
    let Some(device) = device::store_device() else { return 0 };
    let Ok(record) = kl_store::record::load(&launcher.join("store/installed.json")) else { return 0 };
    let Ok(key) = device::store_catalog_key() else { return 0 };
    let Ok(catalog) = refresh::cached_catalog(&launcher.join("store/cache"), key.as_ref()) else { return 0 };
    record.apps.iter().filter(|(id, installed)| {
        let Ok(listing) = catalog.listing(id) else { return false };
        let Ok(version) = resolve::pick_version(&listing, &device) else { return false };
        let current = kl_store::Version::parse(&installed.version);
        current.is_some() && kl_store::Version::parse(&version.version) > current
    }).count()
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
    report("Finding Store…");
    let key = device::store_catalog_key().map_err(|error| format!("The Store signing policy could not be read. {error}"))?;
    let (listing, asset) = bootstrap_release(CATALOG, &NetSource, key.as_ref(), &device)?;
    let available = device::available_storage_bytes(root)
        .map_err(|_| "Available Kindle storage could not be checked.")?;
    if available / 3 < asset.size {
        return Err("Not enough space. Free some Kindle storage and try again.".into());
    }

    let paths = Paths::under(root);
    let downloads = root.join("launcher/store/downloads");
    fs::create_dir_all(&downloads).map_err(|e| e.to_string())?;
    let zip = downloads.join("primitive-store-bootstrap.zip");
    report("Downloading Store…");
    primitive_net::download_resumable(
        &asset.url,
        &zip,
        &primitive_net::Policy::store(asset.size),
        asset.size,
        &asset.sha256,
        |_, _| {},
    )
    .map_err(|e| format!("Download failed. Check Wi-Fi and try again. {e}"))?;
    report("Checking and installing Store…");
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

fn bootstrap_release(
    base: &str,
    source: &dyn refresh::Source,
    key: Option<&TrustedKey>,
    device: &resolve::Device,
) -> Result<(Listing, Asset), String> {
    let listing = check_listing(refresh::fetch_listing(base, ID, source, key)?)?;
    let version = resolve::pick_version(&listing, device).map_err(|error| error.to_string())?;
    let asset = version.assets[&device.arch].clone();
    kl_store::verify::check_archive_size(asset.size).map_err(|error| error.to_string())?;
    if let Some(key) = key {
        refresh::verify_asset_signature(source, key, ID, &version.version, &device.arch, &asset)?;
    }
    Ok((listing, asset))
}

fn check_listing(listing: Listing) -> Result<Listing, String> {
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
    use kl_core::{Action, Input, Launch, Layout, Screen, Ui};
    use std::os::unix::fs::{symlink, PermissionsExt};
    use std::path::PathBuf;

    const FIXTURE_CATALOG: &str = "https://catalog.invalid/v1";

    struct FixtureCatalog {
        site: PathBuf,
        tampered: bool,
    }

    impl refresh::Source for FixtureCatalog {
        fn text(&self, url: &str) -> Result<String, String> {
            String::from_utf8(self.bytes(url, refresh::MAX_INDEX)?).map_err(|error| error.to_string())
        }

        fn bytes(&self, url: &str, max: u64) -> Result<Vec<u8>, String> {
            use std::io::Read;

            let relative = url.strip_prefix("https://catalog.invalid/v1/").ok_or("unexpected fixture URL")?;
            if relative.split('/').any(|part| part.is_empty() || part == "." || part == "..") {
                return Err("invalid fixture URL".into());
            }
            if self.tampered && relative.ends_with(".zip.sig") {
                return Ok(b"bad proof".to_vec());
            }
            let mut bytes = Vec::new();
            fs::File::open(self.site.join(relative)).map_err(|error| error.to_string())?
                .take(max + 1).read_to_end(&mut bytes).map_err(|error| error.to_string())?;
            if bytes.len() as u64 > max {
                return Err("the fixture is too large".into());
            }
            Ok(bytes)
        }
    }

    fn fixture(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("kl-bootstrap-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(path.join("documents")).unwrap();
        path
    }

    #[test]
    fn discovery_and_home_use_one_store_tile_despite_legacy_entries_and_overrides() {
        let root = fixture("tiles");
        let documents = root.join("documents");
        let launcher = root.join("launcher");
        fs::create_dir_all(&launcher).unwrap();
        for (file, name) in [
            (ENTRY, "Primitive Store"),
            ("Primitive Store.sh", "Old Catalog"),
            ("primitive-store.sh", "Launcher Catalog"),
            ("Catalog backup.sh", "Primitive Store"),
            ("Inventory.sh", "Store Inventory"),
        ] {
            fs::write(documents.join(file), format!("#!/bin/sh\n# Name: {name}\n")).unwrap();
        }
        fs::write(launcher.join("apps.toml"), r#"
[[app]]
id = "store"
name = "Integrated catalog"
mode = "background"
[[app]]
id = "Catalog backup.sh"
name = "Renamed backup"
[[app]]
id = "old-catalog"
name = " store "
command = "echo backup"
[[app]]
id = "Store.sh"
name = "Renamed Store"
command = "echo override"
dir = "/tmp"
mode = "background"
"#).unwrap();
        let home_store = |expected_id| {
            let (_, apps, _, problems) = crate::load_library(&launcher, &documents);
            assert!(problems.is_empty(), "{problems:?}");
            assert_eq!(apps.len(), 3, "Settings, the Store and Inventory (the Kindle UI is in the power menu)");
            assert!(apps.iter().any(|app| app.id == "Inventory.sh"));
            let index = apps.iter().position(|app| app.id == expected_id).unwrap();
            let mut ui = Ui::new(apps, Layout::new(1264, 1680, 4, 5));
            let rect = ui.icon_rect(index).unwrap();
            let tap = Input::Tap(rect.x as f32 + 1.0, rect.y as f32 + 1.0);
            let Action::Launch(app) = ui.handle(tap) else { panic!("Store should launch") };
            assert_eq!(ui.screen, Screen::Home);
            assert_eq!(app.name, "Store");
            assert!(!app.background);
            app
        };
        let built_in = home_store("store");
        assert_eq!(built_in.launch, Launch::Store);
        assert!(entry_for(built_in.clone(), &documents).is_none());

        let binary = root.join(BINARY);
        fs::create_dir_all(binary.parent().unwrap()).unwrap();
        fs::write(&binary, "program").unwrap();
        assert!(!store_present(&documents));
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();
        let script = documents.join(ENTRY);
        let descriptor = format!("{}\nscript\n{}\n", documents.display(), script.display());
        for name in ["Primitive Store", "Store"] {
            fs::write(&script, format!("#!/bin/sh\n# Name: {name}\n")).unwrap();
            let standalone = home_store(ENTRY);
            assert_eq!(standalone.next_file().as_deref(), Some(descriptor.as_str()));
            let app = entry_for(built_in.clone(), &documents).unwrap();
            assert_eq!(app.next_file().as_deref(), Some(descriptor.as_str()));
        }
        let active = home_store(ENTRY);
        fs::remove_file(&script).unwrap();
        assert!(entry_for(active, &documents).is_none());
        symlink(documents.join("Primitive Store.sh"), script).unwrap();
        assert!(!store_present(&documents));
        assert_eq!(home_store("store").launch, Launch::Store);
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
            "schema": 1, "id": ID, "dir": DIR, "name": "Store",
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
        let device = resolve::Device {
            arch: manifest.arch,
            firmware: kl_store::Version::parse("5.18.2").unwrap(),
        };
        let listing = if let Some(site) = std::env::var_os("KL_STORE_BOOTSTRAP_CATALOG") {
            let trust = std::env::var_os("KL_TEST_CATALOG_PUBLIC_KEY").unwrap();
            let key = TrustedKey::from_hex(fs::read_to_string(trust).unwrap().trim()).unwrap();
            let mut source = FixtureCatalog { site: PathBuf::from(site), tampered: false };
            let (listing, _) = bootstrap_release(FIXTURE_CATALOG, &source, Some(&key), &device).unwrap();
            source.tampered = true;
            assert!(bootstrap_release(FIXTURE_CATALOG, &source, Some(&key), &device).is_err());
            println!("Launcher authenticated the staged Store archive and rejected its tampered signature");
            listing
        } else {
            check_listing(serde_json::from_str(&text).unwrap()).unwrap()
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
