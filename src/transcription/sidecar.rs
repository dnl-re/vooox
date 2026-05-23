use crate::storage::paths;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, ChildStdout, Command, Stdio};

pub struct SidecarProcess {
    pub child: Child,
    pub port: u16,
}

pub fn start_whisper_sidecar() -> Result<SidecarProcess, String> {
    let server_script = find_whisper_server_script()?;
    let mut child = launch_python_process(&server_script)?;
    let port = read_announced_port_from_stdout(&mut child)?;
    Ok(SidecarProcess { child, port })
}

fn find_whisper_server_script() -> Result<PathBuf, String> {
    let candidates = [
        PathBuf::from("whisper_server/server.py"),
        path_next_to_executable("../whisper_server/server.py"),
    ];
    candidates
        .into_iter()
        .find(|path| path.exists())
        .ok_or_else(|| "whisper_server/server.py not found".to_string())
}

fn path_next_to_executable(relative: &str) -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(relative)))
        .unwrap_or_default()
}

fn launch_python_process(server_script: &PathBuf) -> Result<Child, String> {
    let python = pick_python_interpreter();
    Command::new(&python)
        .arg(server_script)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("could not start sidecar ({}): {e}", python.display()))
}

fn pick_python_interpreter() -> PathBuf {
    if paths::venv_python_exists() {
        paths::venv_python()
    } else {
        PathBuf::from("python3")
    }
}

fn read_announced_port_from_stdout(child: &mut Child) -> Result<u16, String> {
    let stdout = child.stdout.take().unwrap();
    let first_line = read_first_line(stdout)?;
    parse_vooox_port_announcement(&first_line)
}

fn read_first_line(stdout: ChildStdout) -> Result<String, String> {
    let mut line = String::new();
    BufReader::new(stdout)
        .read_line(&mut line)
        .map_err(|e| format!("sidecar stdout: {e}"))?;
    Ok(line)
}

fn parse_vooox_port_announcement(line: &str) -> Result<u16, String> {
    line.trim()
        .strip_prefix("VOOOX_PORT=")
        .and_then(|port| port.parse().ok())
        .ok_or_else(|| format!("unexpected sidecar output: {line:?}"))
}
