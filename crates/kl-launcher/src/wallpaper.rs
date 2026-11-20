//! User-storage wallpaper preferences and images. Keep selections in their own file so older
//! Store versions can continue rewriting the legacy Launcher settings without losing them.

use std::{
    fs::{self, OpenOptions},
    io::{self, Read},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};

use kl_core::wallpaper::{WallpaperChoice, WallpaperSettings, WallpaperTarget};
use kl_kindle::{device, log};
use kl_render::{image::{self, GrayImage}, Renderer};

const MAX_IMAGE_BYTES: u64 = 20 * 1024 * 1024;
const MAX_PREFS_BYTES: u64 = 4 * 1024;
const GALLERY: &str = "wallpapers";
const LEGACY_GALLERY: &str = "screensavers";

pub fn preferences_path(dir: &Path) -> PathBuf {
    dir.join("wallpaper-settings.txt")
}

pub fn load(dir: &Path, legacy_sleep: bool) -> WallpaperSettings {
    let preferences = read_regular(&preferences_path(dir), MAX_PREFS_BYTES)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .unwrap_or_default();
    let mut wallpapers = parse(&preferences, legacy_sleep);
    wallpapers.images = png_names(&dir.join(GALLERY));
    for target in [WallpaperTarget::Home, WallpaperTarget::Sleep, WallpaperTarget::Startup] {
        if let WallpaperChoice::File(name) = wallpapers.selection(target) {
            if !wallpapers.images.iter().any(|candidate| candidate == name) {
                wallpapers.set_selection(target, WallpaperChoice::Default);
            }
        }
    }
    wallpapers
}

fn parse(input: &str, legacy_sleep: bool) -> WallpaperSettings {
    let mut wallpapers = WallpaperSettings::default();
    if legacy_sleep {
        wallpapers.sleep = WallpaperChoice::Rotate;
    }
    for (key, value) in input.lines().filter_map(|line| line.split_once('=')) {
        let opacity_setting = key.strip_suffix("-opacity");
        let target = match opacity_setting.unwrap_or(key) {
            "home" => WallpaperTarget::Home,
            "sleep" => WallpaperTarget::Sleep,
            "startup" => WallpaperTarget::Startup,
            _ => continue,
        };
        if opacity_setting.is_some() {
            if let Ok(opacity @ 0..=100) = value.parse::<u8>() {
                wallpapers.set_opacity(target, opacity);
            }
            continue;
        }
        let choice = match value {
            "default" => WallpaperChoice::Default,
            "rotate" if target == WallpaperTarget::Sleep => WallpaperChoice::Rotate,
            name if name.strip_prefix("file:").is_some_and(valid_name) => {
                WallpaperChoice::File(name[5..].into())
            }
            _ => continue,
        };
        wallpapers.set_selection(target, choice);
    }
    wallpapers
}

pub fn preferences(wallpapers: &WallpaperSettings) -> String {
    let name = |choice: &WallpaperChoice| match choice {
        WallpaperChoice::Default => "default".into(),
        WallpaperChoice::Rotate => "rotate".into(),
        WallpaperChoice::File(name) => format!("file:{name}"),
    };
    format!(
        "home={}\nsleep={}\nstartup={}\nhome-opacity={}\nsleep-opacity={}\nstartup-opacity={}\n",
        name(&wallpapers.home), name(&wallpapers.sleep), name(&wallpapers.startup),
        wallpapers.opacity(WallpaperTarget::Home), wallpapers.opacity(WallpaperTarget::Sleep),
        wallpapers.opacity(WallpaperTarget::Startup),
    )
}

fn valid_name(name: &str) -> bool {
    !name.starts_with('.')
        && !name.contains(['/', '\\', ':'])
        && !name.chars().any(char::is_control)
        && Path::new(name).extension().is_some_and(|extension| extension.eq_ignore_ascii_case("png"))
}

fn real_directory(dir: &Path) -> bool {
    fs::symlink_metadata(dir).is_ok_and(|metadata| metadata.is_dir())
}

fn png_names(dir: &Path) -> Vec<String> {
    if !real_directory(dir) {
        return Vec::new();
    }
    let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };
    let mut names: Vec<_> = entries.flatten().filter_map(|entry| {
        let name = entry.file_name().into_string().ok()?;
        (valid_name(&name) && entry.file_type().is_ok_and(|kind| kind.is_file())).then_some(name)
    }).collect();
    names.sort();
    names
}

fn read_regular(path: &Path, max_size: u64) -> io::Result<Vec<u8>> {
    let file = OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK).open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Expected a regular file."));
    }
    if metadata.len() > max_size {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "File is too large."));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(max_size + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max_size {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "File is too large."));
    }
    Ok(bytes)
}

fn picture(dir: &Path, name: &str, target: WallpaperTarget, w: u32, h: u32) -> io::Result<GrayImage> {
    if !valid_name(name) || !real_directory(dir) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Wallpaper folder or PNG filename is invalid."));
    }
    let bytes = read_regular(&dir.join(name), MAX_IMAGE_BYTES)?;
    let image = if target == WallpaperTarget::Home {
        image::wallpaper_tile(&bytes, w, h)
    } else {
        image::picture_tile(&bytes, w, h)
    };
    image.map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

/// Prepare a fixed picture before updating preferences or a display buffer. Neither the stock
/// screensaver state nor the previous selection changes if reading or decoding this file fails.
pub fn selected_picture(
    dir: &Path, target: WallpaperTarget, choice: &WallpaperChoice, next: &mut usize,
) -> io::Result<Option<GrayImage>> {
    match choice {
        WallpaperChoice::File(name) => picture(&dir.join(GALLERY), name, target, super::W, super::H).map(Some),
        WallpaperChoice::Default => Ok(None),
        WallpaperChoice::Rotate if target == WallpaperTarget::Sleep => Ok(rotated_picture(dir, next)),
        WallpaperChoice::Rotate => Err(io::Error::new(io::ErrorKind::InvalidInput, "Rotation applies to Sleep only.")),
    }
}

pub fn prepared_picture(
    dir: &Path, target: WallpaperTarget, wallpapers: &WallpaperSettings, next: &mut usize,
) -> io::Result<Option<GrayImage>> {
    let mut picture = selected_picture(dir, target, wallpapers.selection(target), next)?;
    // Home blends into its themed interface. The Sleep and Startup canvases are white.
    if target != WallpaperTarget::Home {
        if let Some(image) = picture.as_mut() {
            image::blend_wallpaper(image, wallpapers.opacity(target));
        }
    }
    Ok(picture)
}

pub fn saved_picture(
    dir: &Path, target: WallpaperTarget, wallpapers: &WallpaperSettings, next: &mut usize,
) -> Option<GrayImage> {
    match prepared_picture(dir, target, wallpapers, next) {
        Ok(image) => image,
        Err(error) => {
            log(&format!("{target:?} wallpaper: {error}"));
            None
        }
    }
}

/// Prepare the next valid legacy picture before sleep. Missing and damaged files never suppress
/// the stock screensaver when no Launcher picture can be drawn.
fn rotated_picture(dir: &Path, next: &mut usize) -> Option<GrayImage> {
    let folder = dir.join(LEGACY_GALLERY);
    let names = png_names(&folder);
    if names.is_empty() {
        *next = 0;
        return None;
    }
    for _ in 0..names.len() {
        let index = *next % names.len();
        *next = index + 1;
        match picture(&folder, &names[index], WallpaperTarget::Sleep, super::W, super::H) {
            Ok(image) => return Some(image),
            Err(error) => log(&format!("rotating sleep picture: {error}")),
        }
    }
    None
}

pub fn refresh_images(dir: &Path, run: &Path, wallpapers: &WallpaperSettings, renderer: &mut Renderer) -> usize {
    renderer.set_home_wallpaper(saved_picture(dir, WallpaperTarget::Home, wallpapers, &mut 0));
    let mut next = 0;
    let sleep = saved_picture(dir, WallpaperTarget::Sleep, wallpapers, &mut next);
    use_launcher_sleep(run, sleep.is_some());
    renderer.set_sleep_image(sleep);
    next
}

/// Restore only the normal screensaver module we unloaded. The marker survives returning
/// from apps, so Default can restore it even after Launcher itself has restarted.
pub fn use_launcher_sleep(run: &Path, enabled: bool) {
    let ownership = run.join("stock-screensaver-unloaded");
    if enabled {
        if device::stock_screensaver_loaded() && fs::write(&ownership, b"").is_ok()
            && !device::set_stock_screensaver(false)
        {
            let _ = fs::remove_file(&ownership);
        }
    } else if ownership.exists()
        && (device::stock_screensaver_loaded() || device::set_stock_screensaver(true))
    {
        let _ = fs::remove_file(&ownership);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_legacy_rotation_and_round_trips_selections() {
        assert_eq!(parse("", true).sleep, WallpaperChoice::Rotate);
        assert_eq!(parse("", false).sleep, WallpaperChoice::Default);
        let parsed = parse("home=file:River=bank.PNG\nsleep=default\nstartup=file:Dawn.png\n", true);
        assert_eq!(parsed.home, WallpaperChoice::File("River=bank.PNG".into()));
        assert_eq!(parsed.sleep, WallpaperChoice::Default);
        assert_eq!(parsed.startup, WallpaperChoice::File("Dawn.png".into()));
        let reloaded = parse(&preferences(&parsed), false);
        assert_eq!(reloaded.home, parsed.home);
        assert_eq!(reloaded.sleep, parsed.sleep);
        assert_eq!(reloaded.startup, parsed.startup);
    }

    #[test]
    fn opacity_round_trips_independently_and_legacy_values_default_to_full() {
        let values = parse("home=file:River.png\nhome-opacity=45\nsleep-opacity=0\nstartup-opacity=110\n", false);
        assert_eq!(values.opacity(WallpaperTarget::Home), 45);
        assert_eq!(values.opacity(WallpaperTarget::Sleep), 0);
        assert_eq!(values.opacity(WallpaperTarget::Startup), 100);
        assert_eq!(parse(&preferences(&values), false), values);
        for target in [WallpaperTarget::Home, WallpaperTarget::Sleep, WallpaperTarget::Startup] {
            assert_eq!(parse("", true).opacity(target), 100);
        }
    }

    #[test]
    fn rejects_invalid_paths_and_rotation_outside_sleep() {
        let parsed = parse("home=file:../photo.png\nsleep=rotate\nstartup=rotate\n", false);
        assert_eq!(parsed.home, WallpaperChoice::Default);
        assert_eq!(parsed.sleep, WallpaperChoice::Rotate);
        assert_eq!(parsed.startup, WallpaperChoice::Default);
        for invalid in [".hidden.png", "/image.png", "parent\\image.png", "bad\nfile.png", "folder/image.png", "image.jpg"] {
            assert!(!valid_name(invalid), "{invalid:?}");
        }
    }

    #[test]
    fn lists_pngs_and_rejects_symlinks_nonfiles_and_oversize_data() {
        let folder = std::env::temp_dir().join(format!("kl-wallpaper-{}", std::process::id()));
        fs::create_dir(&folder).unwrap();
        let first = folder.join("A.png");
        fs::write(&first, b"12345").unwrap();
        fs::write(folder.join("z.PNG"), b"a").unwrap();
        std::os::unix::fs::symlink(&first, folder.join("link.png")).unwrap();
        fs::create_dir(folder.join("directory.png")).unwrap();
        fs::write(folder.join("._A.png"), b"a").unwrap();
        assert_eq!(png_names(&folder), ["A.png", "z.PNG"]);
        assert_eq!(read_regular(&first, 5).unwrap(), b"12345");
        assert!(read_regular(&first, 4).is_err());
        assert!(read_regular(&folder.join("link.png"), 5).is_err());
        assert!(read_regular(&folder.join("directory.png"), 5).is_err());
        fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn damaged_wallpapers_fall_back_and_rotation_skips_broken_pictures() {
        let dir = std::env::temp_dir().join(format!("kl-bad-wallpaper-{}", std::process::id()));
        fs::create_dir(&dir).unwrap();
        let folder = dir.join(LEGACY_GALLERY);
        fs::create_dir(&folder).unwrap();
        fs::write(folder.join("A.png"), b"broken PNG").unwrap();
        let mut next = 0;
        let mut wallpapers = parse("", true);
        assert!(saved_picture(&dir, WallpaperTarget::Sleep, &wallpapers, &mut next).is_none());
        wallpapers.sleep = WallpaperChoice::File("Missing.png".into());
        assert!(saved_picture(&dir, WallpaperTarget::Sleep, &wallpapers, &mut next).is_none());
        let icon = include_bytes!("../../../assets/launcher-icon.png");
        fs::write(folder.join("B.png"), icon).unwrap();
        wallpapers.sleep = WallpaperChoice::Rotate;
        wallpapers.set_opacity(WallpaperTarget::Sleep, 0);
        let faded = saved_picture(&dir, WallpaperTarget::Sleep, &wallpapers, &mut next).unwrap();
        assert!(faded.pixels.iter().all(|&pixel| pixel == 255));
        assert_eq!(next, 2);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn unplugging_after_a_file_is_deleted_restores_the_default_selection() {
        let dir = std::env::temp_dir().join(format!("kl-eject-wallpaper-{}", std::process::id()));
        fs::create_dir(&dir).unwrap();
        let folder = dir.join(GALLERY);
        fs::create_dir(&folder).unwrap();
        fs::write(preferences_path(&dir), "home=file:Coast.png\nsleep=file:Peak.png\n").unwrap();
        fs::write(folder.join("Peak.png"), b"listed but not decoded yet").unwrap();
        let wallpapers = load(&dir, true);
        assert_eq!(wallpapers.home, WallpaperChoice::Default);
        assert_eq!(wallpapers.sleep, WallpaperChoice::File("Peak.png".into()));
        fs::remove_dir_all(dir).unwrap();
    }
}
