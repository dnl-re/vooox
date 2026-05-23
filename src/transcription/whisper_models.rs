//! Metadaten und lokale Cache-Erkennung der faster-whisper-Modelle.
//!
//! Größen-/Zeitangaben sind grobe Schätzwerte (Stand 2025) und werden im
//! Setup-Wizard und in den Settings angezeigt, damit der User weiß was er sich
//! einhandelt bevor er auf "Herunterladen" klickt.

use std::path::PathBuf;

#[derive(Debug, Clone, Copy)]
pub struct ModelInfo {
    pub id: &'static str,
    pub size_mb: u32,
}

pub const MODELS: &[ModelInfo] = &[
    ModelInfo { id: "tiny",     size_mb: 75   },
    ModelInfo { id: "base",     size_mb: 145  },
    ModelInfo { id: "small",    size_mb: 480  },
    ModelInfo { id: "medium",   size_mb: 1500 },
    ModelInfo { id: "large-v2", size_mb: 3000 },
    ModelInfo { id: "large-v3", size_mb: 3000 },
];

pub fn info(id: &str) -> Option<ModelInfo> {
    MODELS.iter().copied().find(|m| m.id == id)
}

/// Geschätzte Downloaddauer bei 100 Mbit/s (~10 MB/s nutzbar) — als
/// menschenlesbarer String.
fn duration_at_100mbit(size_mb: u32) -> String {
    let seconds = size_mb as f32 / 10.0;
    if seconds < 60.0 {
        "unter 1 min".into()
    } else {
        format!("ca. {} min", (seconds / 60.0).round() as u32)
    }
}

pub fn size_label(id: &str) -> String {
    match info(id) {
        Some(m) => format!("{} · {} bei 100 Mbit", format_file_size(m.size_mb), duration_at_100mbit(m.size_mb)),
        None => "unbekannte Größe".into(),
    }
}

/// Kompakte Größenangabe ohne Dauer — für die Combobox, damit die Zeile dort
/// nicht überlang wird.
pub fn size_label_short(id: &str) -> String {
    match info(id) {
        Some(m) => format_file_size(m.size_mb),
        None => "?".into(),
    }
}

fn format_file_size(size_mb: u32) -> String {
    if size_mb >= 1000 {
        format!("ca. {:.1} GB", size_mb as f32 / 1000.0)
    } else {
        format!("ca. {} MB", size_mb)
    }
}

fn hf_hub_root() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join(".cache").join("huggingface").join("hub")
}

/// Returns the cache directory for a given model id. faster-whisper pulls from
/// `Systran/faster-whisper-<id>` on HuggingFace, which HF caches as
/// `models--Systran--faster-whisper-<id>`.
pub fn cache_dir(id: &str) -> PathBuf {
    hf_hub_root().join(format!("models--Systran--faster-whisper-{id}"))
}

pub fn is_downloaded(id: &str) -> bool {
    let dir = cache_dir(id);
    if !dir.is_dir() {
        return false;
    }
    // HF stores blobs under `snapshots/<rev>/`. Require at least one snapshot
    // to consider the model usable.
    let snaps = dir.join("snapshots");
    snaps.is_dir() && snaps.read_dir().map(|mut it| it.next().is_some()).unwrap_or(false)
}

pub fn delete_cache(id: &str) -> std::io::Result<()> {
    let dir = cache_dir(id);
    if dir.exists() {
        std::fs::remove_dir_all(dir)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_models_listed() {
        assert!(info("small").is_some());
        assert!(info("large-v3").is_some());
        assert!(info("bogus").is_none());
    }

    #[test]
    fn size_label_formats() {
        assert!(size_label("small").contains("MB"));
        assert!(size_label("large-v3").contains("GB"));
    }

    #[test]
    fn cache_dir_has_systran_prefix() {
        let p = cache_dir("small");
        assert!(p.to_string_lossy().contains("models--Systran--faster-whisper-small"));
    }
}
