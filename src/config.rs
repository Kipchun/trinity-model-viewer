//! Remembers the RomFS folder the user picked, in a one-line text file in the user's config directory
//! (%APPDATA%\trinity-model-viewer on Windows, $XDG_CONFIG_HOME or ~/.config/trinity-model-viewer elsewhere).

use std::path::PathBuf;

fn file() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("trinity-model-viewer").join("romfs_path.txt"))
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
