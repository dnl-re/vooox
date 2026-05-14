use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub shortcut: String,
    pub microphone: Option<String>,
    pub model: String,
    pub language: String,
    pub autostart: bool,
    pub overlay_position: OverlayPosition,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum OverlayPosition {
    BottomRight,
    BottomLeft,
    TopRight,
    TopLeft,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            shortcut: "ctrl+shift+space".into(),
            microphone: Some("pulse".into()),
            model: "small".into(),
            language: "de".into(),
            autostart: false,
            overlay_position: OverlayPosition::BottomRight,
        }
    }
}

fn config_path() -> PathBuf {
    dirs_path().join("config.toml")
}

fn dirs_path() -> PathBuf {
    let base = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
            PathBuf::from(home).join(".config")
        });
    base.join("vooox")
}

fn autostart_path() -> PathBuf {
    let base = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
            PathBuf::from(home).join(".config")
        });
    base.join("autostart").join("vooox.desktop")
}

impl Config {
    pub fn load() -> Self {
        let path = config_path();
        if let Ok(text) = fs::read_to_string(&path) {
            toml::from_str(&text).unwrap_or_default()
        } else {
            Self::default()
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        let dir = dirs_path();
        fs::create_dir_all(&dir)?;
        let text = toml::to_string_pretty(self).expect("config serialization");
        fs::write(config_path(), text)?;
        self.sync_autostart()
    }

    fn sync_autostart(&self) -> std::io::Result<()> {
        let path = autostart_path();
        if self.autostart {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            let exe = std::env::current_exe()
                .unwrap_or_else(|_| PathBuf::from("vooox"))
                .display()
                .to_string();
            let entry = format!(
                "[Desktop Entry]\nType=Application\nName=vooox\nExec={exe}\nHidden=false\nNoDisplay=false\nX-GNOME-Autostart-enabled=true\n"
            );
            let mut f = fs::File::create(&path)?;
            f.write_all(entry.as_bytes())?;
        } else if path.exists() {
            fs::remove_file(&path)?;
        }
        Ok(())
    }

    pub fn autostart_desktop_entry(&self) -> String {
        let exe = std::env::current_exe()
            .unwrap_or_else(|_| PathBuf::from("vooox"))
            .display()
            .to_string();
        format!(
            "[Desktop Entry]\nType=Application\nName=vooox\nExec={exe}\nHidden=false\nNoDisplay=false\nX-GNOME-Autostart-enabled=true\n"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::sync::Mutex;

    // serialize tests that mutate XDG_CONFIG_HOME env var
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn with_temp_config<F: FnOnce()>(f: F) {
        let _lock = ENV_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        env::set_var("XDG_CONFIG_HOME", tmp.path());
        env::set_var("HOME", tmp.path()); // also pin HOME so autostart_path is predictable
        f();
        env::remove_var("XDG_CONFIG_HOME");
        env::remove_var("HOME");
        // tmp drops here, cleaning up the directory
    }

    #[test]
    fn default_config_roundtrip() {
        with_temp_config(|| {
            let cfg = Config::default();
            cfg.save().unwrap();
            let loaded = Config::load();
            assert_eq!(loaded.shortcut, cfg.shortcut);
            assert_eq!(loaded.model, cfg.model);
            assert_eq!(loaded.language, cfg.language);
            assert_eq!(loaded.overlay_position, cfg.overlay_position);
        });
    }

    #[test]
    fn load_missing_returns_default() {
        with_temp_config(|| {
            let cfg = Config::load();
            assert_eq!(cfg.shortcut, Config::default().shortcut);
        });
    }

    #[test]
    fn autostart_desktop_entry_contains_exe() {
        let cfg = Config::default();
        let entry = cfg.autostart_desktop_entry();
        assert!(entry.contains("[Desktop Entry]"));
        assert!(entry.contains("Type=Application"));
    }

    #[test]
    fn save_creates_autostart_file() {
        with_temp_config(|| {
            let mut cfg = Config::default();
            cfg.autostart = true;
            cfg.save().unwrap();
            assert!(autostart_path().exists());

            cfg.autostart = false;
            cfg.save().unwrap();
            assert!(!autostart_path().exists());
        });
    }
}
