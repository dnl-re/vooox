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

use crate::storage::paths;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::mpsc;

const MINIMUM_CUDA12_DRIVER_MAJOR: u32 = 525;

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
        None => NvidiaHardware::NoDriver,
        Some(driver) => classify_driver_version(driver),
    }
}

fn classify_driver_version(driver: String) -> NvidiaHardware {
    let major = parse_driver_major_version(&driver);
    if major < MINIMUM_CUDA12_DRIVER_MAJOR {
        NvidiaHardware::DriverTooOld { driver }
    } else {
        NvidiaHardware::Ok { driver }
    }
}

fn parse_driver_major_version(driver: &str) -> u32 {
    driver
        .split('.')
        .next()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0)
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
    let line = first_non_empty_line(&out.stdout)?;
    Some(line)
}

fn first_non_empty_line(output: &[u8]) -> Option<String> {
    let line = String::from_utf8_lossy(output).lines().next()?.trim().to_string();
    if line.is_empty() { None } else { Some(line) }
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
    let lib_dir = paths::venv_dir().join("lib");
    let Ok(entries) = std::fs::read_dir(&lib_dir) else {
        return false;
    };
    entries.flatten().any(|e| cuda_wheels_present_in(&e.path()))
}

fn cuda_wheels_present_in(python_version_dir: &std::path::Path) -> bool {
    let nvidia = python_version_dir.join("site-packages").join("nvidia");
    nvidia.join("cublas").is_dir() && nvidia.join("cudnn").is_dir()
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
    std::thread::spawn(move || run_cuda_pip_install(tx));
}

fn run_cuda_pip_install(tx: mpsc::Sender<InstallMsg>) {
    let pip = paths::venv_dir().join("bin/pip");
    if !pip.exists() {
        let _ = tx.send(InstallMsg::Error(format!("pip nicht gefunden unter {} — Setup zuerst abschließen.", pip.display())));
        return;
    }
    let install_args = build_pip_install_args();
    announce_install_command(&tx, &pip, &install_args);
    let Some(mut child) = start_pip_process_or_report_error(&pip, &install_args, &tx) else { return; };
    stream_process_output_to_channel(&mut child, &tx);
    report_install_exit_status(child.wait(), &tx);
}

fn start_pip_process_or_report_error(
    pip: &std::path::Path,
    args: &[String],
    tx: &mpsc::Sender<InstallMsg>,
) -> Option<std::process::Child> {
    match start_cuda_install_process(pip, args) {
        Ok(child) => Some(child),
        Err(e) => { let _ = tx.send(InstallMsg::Error(e)); None }
    }
}

fn report_install_exit_status(result: std::io::Result<std::process::ExitStatus>, tx: &mpsc::Sender<InstallMsg>) {
    match result {
        Err(e) => { let _ = tx.send(InstallMsg::Error(format!("pip wait: {e}"))); }
        Ok(s) if !s.success() => { let _ = tx.send(InstallMsg::Error(format!("pip install exit {}", s.code().unwrap_or(-1)))); }
        Ok(_) => { let _ = tx.send(InstallMsg::Done); }
    }
}

fn build_pip_install_args() -> Vec<String> {
    let mut args = vec!["install".into()];
    args.extend(CUDA_PACKAGES.iter().map(|&p| p.into()));
    args
}

fn announce_install_command(tx: &mpsc::Sender<InstallMsg>, pip: &std::path::Path, args: &[String]) {
    let _ = tx.send(InstallMsg::Line(format!("$ {} {}", pip.display(), args.join(" "))));
}

fn start_cuda_install_process(
    pip: &std::path::Path,
    args: &[String],
) -> Result<std::process::Child, String> {
    Command::new(pip)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("pip spawn: {e}"))
}

fn stream_process_output_to_channel(child: &mut std::process::Child, tx: &mpsc::Sender<InstallMsg>) {
    let h_out = stream_pipe_lines_to_channel(child.stdout.take().unwrap(), tx.clone());
    let h_err = stream_pipe_lines_to_channel(child.stderr.take().unwrap(), tx.clone());
    let _ = h_out.join();
    let _ = h_err.join();
}

fn stream_pipe_lines_to_channel<R: std::io::Read + Send + 'static>(
    pipe: R,
    tx: mpsc::Sender<InstallMsg>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        for line in BufReader::new(pipe).lines().map_while(Result::ok) {
            let _ = tx.send(InstallMsg::Line(line));
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn driver_too_old_when_under_525() {
        assert_eq!(parse_driver_major_version("470.182.03"), 470);
        assert!(parse_driver_major_version("470.182.03") < MINIMUM_CUDA12_DRIVER_MAJOR);
    }

    #[test]
    fn driver_ok_at_525() {
        assert!(parse_driver_major_version("535.183.06") >= MINIMUM_CUDA12_DRIVER_MAJOR);
    }
}
