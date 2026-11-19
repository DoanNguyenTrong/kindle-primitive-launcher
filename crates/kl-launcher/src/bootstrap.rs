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
use kl_store::catalog::{Asset, Listing};
use kl_store::signatures::TrustedKey;
use kl_store::{install::Paths, pipeline, refresh, resolve};

const CATALOG: &str = "https://doannguyentrong.github.io/kindle-primitive-catalog/v1";
const ID: &str = kl_core::store::STORE_ID;
const DIR: &str = "store";
const BINARY: &str = "store/bin/kl-store-app";

struct NetSource;

impl refresh::Source for NetSource {
    fn text(&self, url: &str) -> Result<String, String> {
        primitive_net::fetch_text(url, &primitive_net::Policy::store(refresh::MAX_INDEX)).map_err(|error| error.to_string())
    }

    fn bytes(&self, url: &str, max: u64) -> Result<Vec<u8>, String> {
        primitive_net::fetch_bytes(url, &primitive_net::Policy::store(max)).map_err(|error| error.to_string())
    }
}

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
    let Ok(key) = device::store_catalog_key() else { return 0 };
    let Ok(catalog) = refresh::cached_catalog(&launcher.join("store/cache"), key.as_ref()) else { return 0 };
    record.apps.iter().filter(|(id, installed)| {
        let Ok(listing) = catalog.listing(id) else { return false };
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
