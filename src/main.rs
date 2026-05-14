mod audio;
mod config;
mod dictation_panel;
mod history;
mod settings;
mod shortcuts;
mod text_inject;
mod tray;
mod whisper_client;

use crate::config::Config;
use crate::dictation_panel::DictationPanel;
use crate::history::History;
use crate::tray::TrayCommand;
use crate::whisper_client::WhisperClient;
use crossbeam_channel::bounded;
use glib;
use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow};
use std::cell::RefCell;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::rc::Rc;

// ── sidecar management ────────────────────────────────────────────────────

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

// ── headless test-pipeline mode ───────────────────────────────────────────

async fn run_test_pipeline_async(wav_path: &str) -> i32 {
    let (mut child, port) = match spawn_sidecar() {
        Ok(x) => x,
        Err(e) => { eprintln!("sidecar error: {e}"); return 1; }
    };
    if let Err(e) = whisper_client::wait_for_ready(port, 60).await {
        eprintln!("{e}");
        let _ = child.kill();
        return 1;
    }
    let wav = match std::fs::read(wav_path) {
        Ok(b) => b,
        Err(e) => { eprintln!("read {wav_path}: {e}"); let _ = child.kill(); return 1; }
    };
    let client = WhisperClient::new(port);
    let result = client.transcribe(&wav, |seg| print!("{seg} ")).await;
    let _ = child.kill();
    match result {
        Ok(full) => { println!("\n--- full: {full}"); 0 }
        Err(e) => { eprintln!("transcribe error: {e}"); 1 }
    }
}

// ── entry point ───────────────────────────────────────────────────────────

fn main() -> glib::ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if let Some(pos) = args.iter().position(|a| a == "--test-pipeline") {
        let path = args.get(pos + 1).cloned().unwrap_or_else(|| {
            eprintln!("usage: vooox --test-pipeline <file.wav>");
            std::process::exit(1);
        });
        let rt = tokio::runtime::Runtime::new().unwrap();
        let code = rt.block_on(run_test_pipeline_async(&path));
        return glib::ExitCode::from(code as u8);
    }

    let app = Application::builder()
        .application_id("de.vooox.app")
        .build();

    app.connect_activate(|app| build_ui(app));
    app.run()
}

// ── GTK app ───────────────────────────────────────────────────────────────

fn build_ui(app: &Application) {
    let config = Rc::new(RefCell::new(Config::load()));

    let (mut sidecar, port) = match spawn_sidecar() {
        Ok(x) => x,
        Err(e) => {
            eprintln!("[main] sidecar failed: {e}");
            show_error_dialog(app, &format!("Sidecar konnte nicht gestartet werden:\n{e}"));
            return;
        }
    };

    let (shortcut_tx, shortcut_rx) = bounded::<()>(8);
    let (tray_tx, tray_rx) = bounded::<TrayCommand>(8);

    let shortcut_str = config.borrow().shortcut.clone();
    match shortcuts::Shortcut::parse(&shortcut_str) {
        Ok(sc) => shortcuts::spawn_listener(sc, shortcut_tx),
        Err(e) => eprintln!("[shortcuts] invalid shortcut '{shortcut_str}': {e}"),
    }

    let tray_handle = tray::spawn_tray(tray_tx);

    let history = Rc::new(RefCell::new(History::load()));
    let panel = Rc::new(DictationPanel::new(app));
    panel.load_history(&history.borrow());

    let recording = Rc::new(RefCell::new(false));
    let recorder: Rc<RefCell<Option<audio::Recorder>>> = Rc::new(RefCell::new(None));

    let device_name = config.borrow().microphone.clone();
    let input_device = device_name
        .as_deref()
        .and_then(audio::find_device_by_name)
        .or_else(audio::default_input_device);

    {
        let panel = Rc::clone(&panel);
        let recording = Rc::clone(&recording);
        let recorder = Rc::clone(&recorder);
        let history = Rc::clone(&history);
        let config = Rc::clone(&config);
        let tray_handle = tray_handle.clone();
        let app_clone = app.clone();

        glib::timeout_add_local(std::time::Duration::from_millis(30), move || {
            // process shortcut events
            while let Ok(()) = shortcut_rx.try_recv() {
                let is_recording = *recording.borrow();
                if !is_recording {
                    if let Some(ref dev) = input_device {
                        match audio::Recorder::start(dev) {
                            Ok(rec) => {
                                eprintln!("[audio] recording started — {}Hz, {}ch",
                                    rec.sample_rate, rec.channels);
                                *recorder.borrow_mut() = Some(rec);
                                *recording.borrow_mut() = true;
                                panel.show_recording(dev);
                                if let Some(ref h) = tray_handle {
                                    tray::set_recording(h, true);
                                }
                            }
                            Err(e) => eprintln!("[audio] start: {e}"),
                        }
                    } else {
                        eprintln!("[audio] no input device — open Settings to configure one");
                    }
                } else {
                    *recording.borrow_mut() = false;
                    panel.show_processing();
                    if let Some(ref h) = tray_handle {
                        tray::set_recording(h, false);
                    }

                    if let Some(rec) = recorder.borrow_mut().take() {
                        let sample_rate = rec.sample_rate;
                        let channels = rec.channels;
                        let samples = rec.stop_and_take();
                        let duration_s = samples.len() as f32 / (sample_rate as f32 * channels as f32);
                        eprintln!("[audio] stopped — {} samples, {:.2}s, {}Hz {}ch",
                            samples.len(), duration_s, sample_rate, channels);
                        let mono = audio::to_mono(&samples, channels);
                        let wav = audio::to_wav_bytes(&mono, sample_rate, 1);
                        eprintln!("[audio] WAV size: {} bytes", wav.len());
                        let cfg = config.borrow().clone();
                        let client = WhisperClient::new(port);
                        let (seg_tx, seg_rx) = bounded::<Result<String, String>>(32);

                        std::thread::spawn(move || {
                            let rt = tokio::runtime::Runtime::new().unwrap();
                            let result = rt.block_on(client.transcribe(&wav, |seg| {
                                eprintln!("[whisper] segment: {seg:?}");
                                let _ = seg_tx.send(Ok(seg));
                            }));
                            if let Err(e) = result {
                                eprintln!("[whisper] error: {e:?}");
                                let _ = seg_tx.send(Err(e));
                            }
                        });

                        let panel2 = Rc::clone(&panel);
                        let history2 = Rc::clone(&history);
                        let full_text = Rc::new(RefCell::new(String::new()));

                        glib::timeout_add_local(
                            std::time::Duration::from_millis(50),
                            move || {
                                loop {
                                    match seg_rx.try_recv() {
                                        Ok(Ok(seg)) => {
                                            panel2.append_segment(&seg);
                                            full_text.borrow_mut().push_str(
                                                &crate::space_join(&full_text.borrow(), &seg),
                                            );
                                        }
                                        Ok(Err(e)) => {
                                            eprintln!("[whisper] {e}");
                                            panel2.finish("", &cfg, &mut history2.borrow_mut());
                                            return glib::ControlFlow::Break;
                                        }
                                        Err(crossbeam_channel::TryRecvError::Empty) => {
                                            return glib::ControlFlow::Continue;
                                        }
                                        Err(crossbeam_channel::TryRecvError::Disconnected) => {
                                            let text = full_text.borrow().clone();
                                            panel2.finish(&text, &cfg, &mut history2.borrow_mut());
                                            return glib::ControlFlow::Break;
                                        }
                                    }
                                }
                            },
                        );
                    }
                }
            }

            // process tray commands
            while let Ok(cmd) = tray_rx.try_recv() {
                match cmd {
                    TrayCommand::OpenSettings => {
                        settings::SettingsWindow::new(&app_clone, Rc::clone(&config), port)
                            .show();
                    }
                    TrayCommand::ShowPanel => {
                        panel.present();
                    }
                    TrayCommand::Quit => app_clone.quit(),
                }
            }

            glib::ControlFlow::Continue
        });
    }

    let _hold = app.hold();

    let pid = sidecar.id() as libc::pid_t;
    std::mem::forget(sidecar);
    app.connect_shutdown(move |_| {
        unsafe { libc::kill(pid, libc::SIGTERM) };
    });
}

fn space_join(existing: &str, seg: &str) -> String {
    if !existing.is_empty() && !existing.ends_with(' ') && !seg.starts_with(' ') {
        format!(" {seg}")
    } else {
        seg.to_string()
    }
}

fn show_error_dialog(app: &Application, msg: &str) {
    let win = ApplicationWindow::builder()
        .application(app)
        .title("vooox — Fehler")
        .build();
    let lbl = gtk4::Label::new(Some(msg));
    lbl.set_margin_top(16);
    lbl.set_margin_bottom(16);
    lbl.set_margin_start(16);
    lbl.set_margin_end(16);
    win.set_child(Some(&lbl));
    win.present();
}
