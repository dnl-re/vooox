//! NVIDIA-GPU-Detection und pip-Install-Routine für CUDA/cuDNN-Wheels.
//!
//! faster-whisper 1.x / CTranslate2 4.5+ ist gegen CUDA 12 + cuDNN 9 gelinkt;
//! es gibt keine andere Wheel-Variante. Damit reduziert sich die Frage
//! „welche Libraries kann diese GPU?" auf einen einzigen Cutoff:
//! unterstützt der NVIDIA-Treiber CUDA 12 (≥ 525.x)?
//!
//! Detection-Reihenfolge:
//!   1. `lspci -nn` → gibt es überhaupt eine NVIDIA-Karte im System?
//!   2. `nvidia-smi --query-gpu=driver_version,...` → läuft der proprietäre
//!      Treiber, und welche Version?
//!   3. Driver-Major < 525 → zu alt für CUDA 12.
//!
//! Installiert werden immer dieselben zwei Wheels (`nvidia-cublas-cu12`,
//! `nvidia-cudnn-cu12`), die ihre eigenen Libraries mitbringen — kein
//! System-CUDA-Toolkit nötig.

use crate::paths;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::mpsc;

/// Wie steht es um die NVIDIA-Hardware in diesem System?
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NvidiaHardware {
    /// Keine NVIDIA-GPU per `lspci` gefunden — GPU-Pfad ist hier nie sinnvoll.
    None,
    /// NVIDIA-Karte vorhanden, aber `nvidia-smi` nicht ausführbar
    /// (Treiber nicht installiert oder Kernel-Modul mismatched).
    NoDriver,
    /// Treiber installiert, aber zu alt für CUDA 12.
    DriverTooOld { driver: String },
    /// Treiber unterstützt CUDA 12 — Wheels können installiert werden.
    Ok { driver: String },
}

pub fn detect_hardware() -> NvidiaHardware {
    if !has_nvidia_pci() {
        return NvidiaHardware::None;
    }
    match query_nvidia_smi() {
        Some(driver) => {
            let major = driver
                .split('.')
                .next()
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(0);
            if major < 525 {
                NvidiaHardware::DriverTooOld { driver }
            } else {
                NvidiaHardware::Ok { driver }
            }
        }
        None => NvidiaHardware::NoDriver,
    }
}

fn has_nvidia_pci() -> bool {
    let out = Command::new("sh")
        .args(["-c", "lspci 2>/dev/null | grep -i nvidia"])
        .output();
    matches!(out, Ok(o) if o.status.success() && !o.stdout.is_empty())
}

fn query_nvidia_smi() -> Option<String> {
    let out = Command::new("nvidia-smi")
        .args(["--query-gpu=driver_version", "--format=csv,noheader"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let line = String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()?
        .trim()
        .to_string();
    if line.is_empty() {
        None
    } else {
        Some(line)
    }
}

/// Prüft, ob ctranslate2 im venv eine CUDA-Device sieht. Das ist die einzig
/// verlässliche Antwort — pip-Pakete können installiert sein, aber bei
/// Treiber-/Library-Mismatch trotzdem failen.
pub fn libs_active_in_venv() -> bool {
    if !paths::venv_python_exists() {
        return false;
    }
    let out = Command::new(paths::venv_python())
        .args([
            "-c",
            "import ctranslate2,sys; sys.exit(0 if ctranslate2.get_cuda_device_count()>0 else 1)",
        ])
        .output();
    matches!(out, Ok(o) if o.status.success())
}

/// Sind die CUDA-Wheels im venv installiert? Schaut nur nach den
/// dist-info-Verzeichnissen — sagt nichts darüber aus, ob sie auch
/// funktionieren (das beantwortet [`libs_active_in_venv`]).
pub fn wheels_installed() -> bool {
    let venv = paths::venv_dir();
    let lib = venv.join("lib");
    let Ok(entries) = std::fs::read_dir(&lib) else {
        return false;
    };
    for e in entries.flatten() {
        let sp = e.path().join("site-packages").join("nvidia");
        if sp.join("cublas").is_dir() && sp.join("cudnn").is_dir() {
            return true;
        }
    }
    false
}

// ── pip install ──────────────────────────────────────────────────────────────

pub enum InstallMsg {
    Line(String),
    Done,
    Error(String),
}

const CUDA_PACKAGES: &[&str] = &["nvidia-cublas-cu12", "nvidia-cudnn-cu12"];

pub fn estimated_download_label() -> &'static str {
    "ca. 2,2 GB"
}

/// Spawnt einen Hintergrund-Thread, der die CUDA-Wheels ins venv installiert
/// und stdout/stderr live über den Channel streamt.
pub fn spawn_cuda_install_thread(tx: mpsc::Sender<InstallMsg>) {
    std::thread::spawn(move || {
        let pip = paths::venv_dir().join("bin/pip");
        if !pip.exists() {
            let _ = tx.send(InstallMsg::Error(format!(
                "pip nicht gefunden unter {} — Setup zuerst abschließen.",
                pip.display()
            )));
            return;
        }
        let mut args: Vec<String> = vec!["install".into()];
        for p in CUDA_PACKAGES {
            args.push((*p).into());
        }
        let _ = tx.send(InstallMsg::Line(format!(
            "$ {} {}",
            pip.display(),
            args.join(" ")
        )));
        let mut child = match Command::new(&pip)
            .args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                let _ = tx.send(InstallMsg::Error(format!("pip spawn: {e}")));
                return;
            }
        };
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let tx_out = tx.clone();
        let tx_err = tx.clone();
        let h_out = std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                let _ = tx_out.send(InstallMsg::Line(line));
            }
        });
        let h_err = std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                let _ = tx_err.send(InstallMsg::Line(line));
            }
        });
        let status = match child.wait() {
            Ok(s) => s,
            Err(e) => {
                let _ = tx.send(InstallMsg::Error(format!("pip wait: {e}")));
                return;
            }
        };
        let _ = h_out.join();
        let _ = h_err.join();
        if !status.success() {
            let _ = tx.send(InstallMsg::Error(format!(
                "pip install exit {}",
                status.code().unwrap_or(-1)
            )));
            return;
        }
        let _ = tx.send(InstallMsg::Done);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn driver_too_old_when_under_525() {
        // direkter Logik-Test: 470 → DriverTooOld
        let driver = "470.182.03".to_string();
        let major = driver
            .split('.')
            .next()
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap();
        assert!(major < 525);
    }

    #[test]
    fn driver_ok_at_525() {
        let driver = "535.183.06".to_string();
        let major: u32 = driver.split('.').next().unwrap().parse().unwrap();
        assert!(major >= 525);
    }
}
