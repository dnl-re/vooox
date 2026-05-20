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
    #[serde(default)]
    pub panel_mode: PanelMode,
    #[serde(default = "default_ptt_enabled")]
    pub push_to_talk_enabled: bool,
    #[serde(default = "default_ptt_threshold_ms")]
    pub push_to_talk_threshold_ms: u32,
    #[serde(default)]
    pub auto_paste_toggle: bool,
    #[serde(default)]
    pub auto_paste_ptt: bool,
}

fn default_ptt_enabled() -> bool {
    true
}

fn default_ptt_threshold_ms() -> u32 {
    500
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum PanelMode {
    Window,
    Icon,
}

impl Default for PanelMode {
    fn default() -> Self {
        PanelMode::Window
    }
}

impl PanelMode {
    pub fn as_str(self) -> &'static str {
        match self {
            PanelMode::Window => "window",
            PanelMode::Icon => "icon",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "window" => Some(PanelMode::Window),
            "icon" => Some(PanelMode::Icon),
            _ => None,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            shortcut: "ctrl+shift+space".into(),
            microphone: Some("pulse".into()),
            model: "small".into(),
            language: "de".into(),
            autostart: false,
            panel_mode: PanelMode::Window,
            push_to_talk_enabled: true,
            push_to_talk_threshold_ms: 500,
            auto_paste_toggle: false,
            auto_paste_ptt: false,
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
