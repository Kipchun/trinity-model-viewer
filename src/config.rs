//! Remembers the RomFS folder the user picked, in a one-line text file in the user's config directory
//! (%APPDATA%\trinity-model-viewer on Windows, $XDG_CONFIG_HOME or ~/.config/trinity-model-viewer elsewhere),
//! and the names the user gave sounds (`sound_names.txt`: one `id<TAB>name` per line).

use std::collections::HashMap;
use std::path::PathBuf;

fn file() -> Option<PathBuf> {
    Some(dir()?.join("romfs_path.txt"))
}

fn dir() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("trinity-model-viewer"))
}

/// The names the user gave sounds, by Wwise id.
pub fn load_sound_names() -> HashMap<u32, String> {
    let Some(text) = dir().and_then(|d| std::fs::read_to_string(d.join("sound_names.txt")).ok())
    else {
        return HashMap::new();
    };
    text.lines()
        .filter_map(|l| {
            let (id, name) = l.split_once('\t')?;
            let name = name.trim();
            (!name.is_empty()).then(|| Some((id.trim().parse().ok()?, name.to_string())))?
        })
        .collect()
}

/// Writes all of them, sorted by id. Returns the error text on failure.
pub fn save_sound_names(names: &HashMap<u32, String>) -> Result<(), String> {
    let d = dir().ok_or("no config folder")?;
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    let mut v: Vec<(&u32, &String)> = names.iter().filter(|(_, n)| !n.trim().is_empty()).collect();
    v.sort();
    let text: String = v
        .iter()
        .map(|(id, n)| format!("{id}\t{}\n", n.replace(['\t', '\n'], " ")))
        .collect();
    std::fs::write(d.join("sound_names.txt"), text).map_err(|e| e.to_string())
}

pub fn load_romfs_path() -> Option<PathBuf> {
    let text = std::fs::read_to_string(file()?).ok()?;
    let p = PathBuf::from(text.trim());
    p.exists().then_some(p)
}

/// Best effort: a failure only means the user picks the folder again next time.
pub fn save_romfs_path(path: &std::path::Path) {
    if let Some(f) = file() {
        if let Some(dir) = f.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(f, path.to_string_lossy().as_bytes());
    }
}
