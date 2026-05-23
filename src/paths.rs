//! XDG-konforme Pfade + Erkennung der vooox-eigenen Python-venv.
//!
//! Die Production-Installation (AppImage) legt eine isolierte Python-Umgebung
//! unter `$XDG_DATA_HOME/vooox/venv` an. Im Dev-Modus (cargo run) gibt es keine
//! venv — dann fallen wir auf System-`python3` zurück.

use std::path::PathBuf;

pub fn data_dir() -> PathBuf {
    xdg_data_home().join("vooox")
}

fn xdg_data_home() -> PathBuf {
    std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home_dir().join(".local").join("share"))
}

fn home_dir() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/tmp".into()))
}

pub fn venv_dir() -> PathBuf {
    data_dir().join("venv")
}

pub fn venv_python() -> PathBuf {
    venv_dir().join("bin").join("python")
}

pub fn setup_marker() -> PathBuf {
    data_dir().join(".setup_complete")
}

/// True if a usable venv exists. A binary alone isn't enough — we also need
/// `faster_whisper` importable inside it; that check happens on-demand because
/// it spawns a Python process.
pub fn venv_python_exists() -> bool {
    venv_python().exists()
}

/// Quick check that `faster_whisper` is importable from our venv. Caller
/// should cache the result; this spawns a subprocess.
pub fn venv_has_faster_whisper() -> bool {
    if !venv_python_exists() {
        return false;
    }
    std::process::Command::new(venv_python())
        .args(["-c", "import faster_whisper"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn setup_is_complete() -> bool {
    setup_marker().exists() && venv_has_faster_whisper()
}

pub fn mark_setup_complete() -> std::io::Result<()> {
    std::fs::create_dir_all(data_dir())?;
    std::fs::write(setup_marker(), b"ok\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn data_dir_uses_xdg_data_home() {
        let _l = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_DATA_HOME", tmp.path());
        assert_eq!(data_dir(), tmp.path().join("vooox"));
        std::env::remove_var("XDG_DATA_HOME");
    }

    #[test]
    fn data_dir_falls_back_to_home() {
        let _l = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        std::env::remove_var("XDG_DATA_HOME");
        std::env::set_var("HOME", tmp.path());
        assert_eq!(data_dir(), tmp.path().join(".local").join("share").join("vooox"));
        std::env::remove_var("HOME");
    }

    #[test]
    fn marker_round_trip() {
        let _l = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_DATA_HOME", tmp.path());
        assert!(!setup_marker().exists());
        mark_setup_complete().unwrap();
        assert!(setup_marker().exists());
        std::env::remove_var("XDG_DATA_HOME");
    }
}
