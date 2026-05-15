use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};

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

    let mut child = Command::new("python3")
        .arg(&server_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("could not start sidecar: {e}"))?;

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
