use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{InstallError, Result};

pub const GAME_DIR_NAME: &str = "でびるコネクショん";

const MACOS_BUNDLE_NAME: &str = "DevilConnection.app";

fn asar_candidates() -> Vec<PathBuf> {
    vec![
        PathBuf::from("resources/app.asar"),
        PathBuf::from(MACOS_BUNDLE_NAME).join("Contents/Resources/app.asar"),
        PathBuf::from("Contents/Resources/app.asar"),
        PathBuf::from("app.asar"),
    ]
}

pub fn locate_asar(path: &Path) -> Result<PathBuf> {
    if path.is_file() {
        return Ok(path.to_path_buf());
    }
    if !path.is_dir() {
        return Err(InstallError::AsarNotFound(path.to_path_buf()));
    }

    for candidate in asar_candidates() {
        let full = path.join(&candidate);
        if full.is_file() {
            return Ok(full);
        }
    }

    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let child = entry.path();
            if child.extension().and_then(|e| e.to_str()) == Some("app") {
                let full = child.join("Contents/Resources/app.asar");
                if full.is_file() {
                    return Ok(full);
                }
            }
        }
    }

    Err(InstallError::AsarNotFound(path.to_path_buf()))
}

pub fn detect_game_dirs() -> Vec<PathBuf> {
    let mut found = Vec::new();

    for library in steam_libraries() {
        let candidate = library.join("steamapps/common").join(GAME_DIR_NAME);
        if candidate.is_dir() && !found.contains(&candidate) {
            found.push(candidate);
        }
    }

    found
}

pub fn detect_game_dir() -> Result<PathBuf> {
    detect_game_dirs()
        .into_iter()
        .find(|dir| locate_asar(dir).is_ok())
        .ok_or(InstallError::GameNotFound)
}

pub fn steam_libraries() -> Vec<PathBuf> {
    let mut roots = steam_roots();

    let mut extra = Vec::new();
    for root in &roots {
        for vdf in [
            root.join("steamapps/libraryfolders.vdf"),
            root.join("config/libraryfolders.vdf"),
        ] {
            let Ok(text) = fs::read_to_string(&vdf) else {
                continue;
            };
            for path in parse_library_folders(&text) {
                let path = PathBuf::from(path);
                if path.is_dir() {
                    extra.push(path);
                }
            }
        }
    }

    for path in extra {
        if !roots.contains(&path) {
            roots.push(path);
        }
    }

    roots
}

#[cfg(target_os = "windows")]
fn steam_roots() -> Vec<PathBuf> {
    let mut roots = vec![
        PathBuf::from("C:/Program Files (x86)/Steam"),
        PathBuf::from("C:/Program Files/Steam"),
    ];

    for drive in "DEFGHIJ".chars() {
        roots.push(PathBuf::from(format!("{drive}:/Steam")));
        roots.push(PathBuf::from(format!("{drive}:/SteamLibrary")));
        roots.push(PathBuf::from(format!("{drive}:/Program Files (x86)/Steam")));
        roots.push(PathBuf::from(format!("{drive}:/Program Files/Steam")));
    }

    roots.retain(|p| p.is_dir());
    roots
}

#[cfg(target_os = "macos")]
fn steam_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(home) = home_dir() {
        roots.push(home.join("Library/Application Support/Steam"));
    }
    roots.retain(|p| p.is_dir());
    roots
}

#[cfg(all(unix, not(target_os = "macos")))]
fn steam_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(home) = home_dir() {
        roots.push(home.join(".local/share/Steam"));
        roots.push(home.join(".steam/steam"));
        roots.push(home.join(".steam/root"));
        roots.push(home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"));
    }
    roots.retain(|p| p.is_dir());
    roots
}

#[cfg(not(target_os = "windows"))]
fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn parse_library_folders(text: &str) -> Vec<String> {
    let mut out = Vec::new();

    for line in text.lines() {
        let mut parts = line.split('"').skip(1);
        let Some(key) = parts.next() else { continue };
        if key != "path" {
            continue;
        }
        let Some(value) = parts.nth(1) else { continue };
        if !value.is_empty() {
            out.push(value.replace("\\\\", "/").replace('\\', "/"));
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_library_paths_from_vdf() {
        let vdf = r#"
"libraryfolders"
{
	"0"
	{
		"path"		"C:\\Program Files (x86)\\Steam"
		"label"		""
		"contentid"		"123"
	}
	"1"
	{
		"path"		"D:\\SteamLibrary"
	}
}
"#;
        assert_eq!(
            parse_library_folders(vdf),
            vec!["C:/Program Files (x86)/Steam", "D:/SteamLibrary"]
        );
    }

    #[test]
    fn ignores_non_path_keys() {
        let vdf = "\t\"label\"\t\t\"my library\"\n\t\"totalsize\"\t\t\"0\"\n";
        assert!(parse_library_folders(vdf).is_empty());
    }

    #[test]
    fn locate_asar_accepts_game_dir_bundle_and_file() {
        let tmp = tempfile::tempdir().unwrap();

        let win = tmp.path().join("win");
        fs::create_dir_all(win.join("resources")).unwrap();
        fs::write(win.join("resources/app.asar"), b"x").unwrap();
        assert_eq!(locate_asar(&win).unwrap(), win.join("resources/app.asar"));

        let mac = tmp.path().join("mac");
        let bundle = mac.join("DevilConnection.app/Contents/Resources");
        fs::create_dir_all(&bundle).unwrap();
        fs::write(bundle.join("app.asar"), b"x").unwrap();
        assert_eq!(locate_asar(&mac).unwrap(), bundle.join("app.asar"));

        assert_eq!(
            locate_asar(&bundle.join("app.asar")).unwrap(),
            bundle.join("app.asar")
        );
    }

    #[test]
    fn locate_asar_finds_differently_named_bundle() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("game");
        let bundle = dir.join("でびるコネクショん.app/Contents/Resources");
        fs::create_dir_all(&bundle).unwrap();
        fs::write(bundle.join("app.asar"), b"x").unwrap();
        assert_eq!(locate_asar(&dir).unwrap(), bundle.join("app.asar"));
    }

    #[test]
    fn locate_asar_reports_missing_archive() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(matches!(
            locate_asar(tmp.path()),
            Err(InstallError::AsarNotFound(_))
        ));
    }
}
