use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

type WindowPosition = (i32, i32);

/// Remembered window position per monitor. Coordinates are physical X11 pixels.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct WindowState {
    #[serde(default)]
    pub positions: HashMap<String, WindowPosition>,
}

fn state_path() -> PathBuf {
    super::paths::data_dir().join("window_state.json")
}

impl WindowState {
    pub fn load() -> Self {
        let path = state_path();
        fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let path = state_path();
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = fs::write(&path, s);
        }
    }

    pub fn set(&mut self, key: String, pos: WindowPosition) {
        self.positions.insert(key, pos);
    }
}

/// Stable identifier for a monitor: connector name if available, else geometry.
pub fn monitor_key(mon: &gtk4::gdk::Monitor) -> String {
    use gtk4::prelude::*;
    if let Some(connector) = non_empty_connector(mon) {
        return connector;
    }
    monitor_geometry_as_key(mon)
}

fn non_empty_connector(mon: &gtk4::gdk::Monitor) -> Option<String> {
    use gtk4::prelude::*;
    let s = mon.connector()?.to_string();
    if s.is_empty() { None } else { Some(s) }
}

fn monitor_geometry_as_key(mon: &gtk4::gdk::Monitor) -> String {
    use gtk4::prelude::*;
    let g = mon.geometry();
    format!("{}x{}+{}+{}", g.width(), g.height(), g.x(), g.y())
}
