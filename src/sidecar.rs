use crate::paths;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

/// Returns the path to the Python interpreter to use for the sidecar.
/// Prefers the project-managed venv (Production / AppImage); falls back to
/// system `python3` when the venv hasn't been set up yet (Dev mode).
pub fn python_command() -> PathBuf {
    if paths::venv_python_exists() {
        paths::venv_python()
    } else {
        PathBuf::from("python3")
    }
}

pub fn spawn_sidecar() -> Result<(Child, u16), String> {
    let candidates = [
        std::path::PathBuf::from("whisper_server/server.py"),
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("../whisper_server/server.py")))
            .unwrap_or_default(),
    ];
    let server_path = candidates
        .iter()
        .find(|p| p.exists())
        .ok_or("whisper_server/server.py not found")?
        .clone();

    let python = python_command();
    let mut child = Command::new(&python)
        .arg(&server_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("could not start sidecar ({}): {e}", python.display()))?;

    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .map_err(|e| format!("sidecar stdout: {e}"))?;
    let port: u16 = line
        .trim()
        .strip_prefix("VOOOX_PORT=")
        .and_then(|p| p.parse().ok())
        .ok_or_else(|| format!("unexpected sidecar output: {line:?}"))?;

    Ok((child, port))
}
