mod audio;
mod storage;
mod system;
mod transcription;
mod ui;

use crate::storage::config::Config;
use crate::storage::history::History;
use crate::transcription::whisper_client::WhisperClient;
use crate::ui::dictation_panel::DictationPanel;
use crate::ui::tray::AppCommand;
use crossbeam_channel::bounded;
use glib;
use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

type StreamRx = crossbeam_channel::Receiver<Option<String>>;

// ── headless test-pipeline mode ───────────────────────────────────────────

async fn run_test_pipeline_async(wav_path: &str) -> i32 {
    let sidecar_process = match transcription::sidecar::start_whisper_sidecar() {
        Ok(x) => x,
        Err(e) => { eprintln!("sidecar error: {e}"); return 1; }
    };
    let port = sidecar_process.port;
    let mut child = sidecar_process.child;
    if let Err(e) = transcription::whisper_client::wait_for_ready(port, 60).await {
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

    let skip_setup = args.iter().any(|a| a == "--skip-setup");

    let app = Application::builder()
        .application_id("de.vooox.app")
        .build();

    app.connect_activate(move |app| {
        if !skip_setup && !storage::paths::setup_is_complete() {
            let app_for_done = app.clone();
            ui::setup_window::show(app, move || build_ui(&app_for_done));
        } else {
            build_ui(app);
        }
    });
    app.run()
}

// ── GTK app ───────────────────────────────────────────────────────────────

fn build_ui(app: &Application) {
    let config = Rc::new(RefCell::new(Config::load()));

    apply_force_cpu_setting(&config.borrow());

    let sidecar_process = match transcription::sidecar::start_whisper_sidecar() {
        Ok(x) => x,
        Err(e) => {
            eprintln!("[main] sidecar failed: {e}");
            show_error_dialog(app, &format!("Sidecar konnte nicht gestartet werden:\n{e}"));
            return;
        }
    };
    let port = sidecar_process.port;
    let sidecar = sidecar_process.child;

    let (shortcut_tx, shortcut_rx) = bounded::<system::shortcuts::ShortcutEvent>(16);
    let (tray_tx, tray_rx) = bounded::<AppCommand>(8);

    start_shortcut_listener(&config.borrow(), shortcut_tx);
    let tray_handle = ui::tray::spawn_tray(tray_tx.clone(), config.borrow().panel_mode);

    let history = Rc::new(RefCell::new(History::load()));
    let panel = Rc::new(DictationPanel::new(app, tray_tx.clone(), Rc::clone(&config)));

    push_saved_config_to_sidecar_once_ready(port, &config.borrow());

    let recording = Rc::new(RefCell::new(false));
    let recorder: Rc<RefCell<Option<audio::Recorder>>> = Rc::new(RefCell::new(None));
    // PTT state: hold_id is incremented on every Press; the threshold timer captures the
    // hold-id at scheduling time and only flips the visual if the same hold is
    // still active when the timer fires.
    let hold_id: Rc<Cell<u64>> = Rc::new(Cell::new(0));
    let ptt_active: Rc<Cell<bool>> = Rc::new(Cell::new(false));

    let input_device = resolve_audio_device_from_config(&config.borrow());

    {
        let panel = Rc::clone(&panel);
        let recording = Rc::clone(&recording);
        let recorder = Rc::clone(&recorder);
        let history = Rc::clone(&history);
        let config = Rc::clone(&config);
        let tray_handle = tray_handle.clone();
        let app_clone = app.clone();

        let hold_id = Rc::clone(&hold_id);
        let ptt_active = Rc::clone(&ptt_active);

        glib::timeout_add_local(std::time::Duration::from_millis(30), move || {
            while let Ok(ev) = shortcut_rx.try_recv() {
                match ev {
                    system::shortcuts::ShortcutEvent::Press => {
                        let this_hold = hold_id.get().wrapping_add(1);
                        hold_id.set(this_hold);
                        ptt_active.set(false);

                        if *recording.borrow() {
                            panel.arm_auto_paste(config.borrow().auto_paste_toggle);
                            stop_recording(
                                Rc::clone(&recorder),
                                Rc::clone(&recording),
                                Rc::clone(&panel),
                                tray_handle.clone(),
                                config.borrow().clone(),
                                Rc::clone(&history),
                                port,
                            );
                        } else if let Some(ref dev) = input_device {
                            start_recording(
                                dev,
                                Rc::clone(&recorder),
                                Rc::clone(&recording),
                                Rc::clone(&panel),
                                tray_handle.clone(),
                                port,
                            );
                            let cfg_snap = config.borrow().clone();
                            if cfg_snap.push_to_talk_enabled {
                                let threshold = std::time::Duration::from_millis(
                                    cfg_snap.push_to_talk_threshold_ms as u64,
                                );
                                let hold_at_schedule = this_hold;
                                let hold_id = Rc::clone(&hold_id);
                                let ptt_active = Rc::clone(&ptt_active);
                                let recording = Rc::clone(&recording);
                                let panel = Rc::clone(&panel);
                                glib::timeout_add_local_once(threshold, move || {
                                    if hold_id.get() == hold_at_schedule
                                        && *recording.borrow()
                                    {
                                        ptt_active.set(true);
                                        panel.set_ptt_active(true);
                                    }
                                });
                            }
                        } else {
                            eprintln!(
                                "[audio] no input device — open Settings to configure one"
                            );
                        }
                    }
                    system::shortcuts::ShortcutEvent::Release => {
                        hold_id.set(hold_id.get().wrapping_add(1));
                        if ptt_active.get() && *recording.borrow() {
                            ptt_active.set(false);
                            panel.set_ptt_active(false);
                            panel.arm_auto_paste(config.borrow().auto_paste_ptt);
                            stop_recording(
                                Rc::clone(&recorder),
                                Rc::clone(&recording),
                                Rc::clone(&panel),
                                tray_handle.clone(),
                                config.borrow().clone(),
                                Rc::clone(&history),
                                port,
                            );
                        }
                        ptt_active.set(false);
                    }
                }
            }

            while let Ok(cmd) = tray_rx.try_recv() {
                match cmd {
                    AppCommand::OpenSettings => {
                        ui::settings::SettingsWindow::new(&app_clone, Rc::clone(&config), port)
                            .show();
                    }
                    AppCommand::ShowPanel => {
                        panel.present();
                    }
                    AppCommand::OpenHistory => {
                        ui::history_window::open(&app_clone, Rc::clone(&history));
                    }
                    AppCommand::HidePanel => {
                        panel.hide();
                    }
                    AppCommand::SetPanelMode(m) => {
                        {
                            let mut cfg = config.borrow_mut();
                            if cfg.panel_mode != m {
                                cfg.panel_mode = m;
                                if let Err(e) = cfg.save() {
                                    eprintln!("[config] save: {e}");
                                }
                            }
                        }
                        panel.apply_mode(m);
                        if let Some(ref h) = tray_handle {
                            ui::tray::set_panel_mode(h, m);
                        }
                    }
                    AppCommand::SetModel(m) => {
                        {
                            let mut cfg = config.borrow_mut();
                            cfg.model = m.clone();
                            if let Err(e) = cfg.save() {
                                eprintln!("[config] save: {e}");
                            }
                        }
                        let language = config.borrow().language.clone();
                        std::thread::spawn(move || {
                            let rt = tokio::runtime::Runtime::new().unwrap();
                            rt.block_on(async move {
                                let client = WhisperClient::new(port);
                                if let Err(e) = client.set_config(&m, &language).await {
                                    eprintln!("[whisper] set_config: {e}");
                                }
                            });
                        });
                    }
                    AppCommand::Quit => app_clone.quit(),
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

// ── setup helpers ─────────────────────────────────────────────────────────

fn apply_force_cpu_setting(cfg: &Config) {
    if cfg.force_cpu {
        std::env::set_var("VOOOX_FORCE_CPU", "1");
    } else {
        std::env::remove_var("VOOOX_FORCE_CPU");
    }
}

fn start_shortcut_listener(
    cfg: &Config,
    shortcut_tx: crossbeam_channel::Sender<system::shortcuts::ShortcutEvent>,
) {
    let shortcut_str = cfg.shortcut.clone();
    match system::shortcuts::Shortcut::parse(&shortcut_str) {
        Ok(sc) => system::shortcuts::spawn_listener(sc, shortcut_tx),
        Err(e) => eprintln!("[shortcuts] invalid shortcut '{shortcut_str}': {e}"),
    }
}

fn push_saved_config_to_sidecar_once_ready(port: u16, cfg: &Config) {
    let model = cfg.model.clone();
    let language = cfg.language.clone();
    let start = std::time::Instant::now();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async move {
            match transcription::whisper_client::wait_for_ready(port, 60).await {
                Ok(_) => {
                    let client = WhisperClient::new(port);
                    if let Err(e) = client.set_config(&model, &language).await {
                        eprintln!("[whisper] initial set_config: {e}");
                    }
                    eprintln!(
                        "[main] vooox ready — model={model} language={language} port={port} startup={:.1}s",
                        start.elapsed().as_secs_f32()
                    );
                }
                Err(e) => eprintln!("[main] sidecar not ready: {e}"),
            }
        });
    });
}

fn resolve_audio_device_from_config(cfg: &Config) -> Option<cpal::Device> {
    cfg.microphone
        .as_deref()
        .and_then(audio::find_device_by_name)
        .or_else(audio::default_input_device)
}

// ── recording state machine ───────────────────────────────────────────────

fn start_recording(
    dev: &cpal::Device,
    recorder: Rc<RefCell<Option<audio::Recorder>>>,
    recording: Rc<RefCell<bool>>,
    panel: Rc<DictationPanel>,
    tray_handle: Option<ksni::blocking::Handle<ui::tray::VoooxTray>>,
    port: u16,
) {
    match audio::Recorder::start(dev) {
        Ok(rec) => {
            eprintln!("[audio] recording started — {}Hz, {}ch", rec.sample_rate, rec.channels);
            *recorder.borrow_mut() = Some(rec);
            *recording.borrow_mut() = true;
            panel.show_recording(dev);
            if let Some(ref h) = tray_handle {
                ui::tray::set_recording(h, true);
            }
            spawn_streaming_timer(Rc::clone(&recorder), Rc::clone(&recording), Rc::clone(&panel), port);
        }
        Err(e) => eprintln!("[audio] start: {e}"),
    }
}

fn stop_recording(
    recorder: Rc<RefCell<Option<audio::Recorder>>>,
    recording: Rc<RefCell<bool>>,
    panel: Rc<DictationPanel>,
    tray_handle: Option<ksni::blocking::Handle<ui::tray::VoooxTray>>,
    cfg: Config,
    history: Rc<RefCell<History>>,
    port: u16,
) {
    *recording.borrow_mut() = false;
    panel.show_processing();
    if let Some(ref h) = tray_handle {
        ui::tray::set_recording(h, false);
    }

    if let Some(rec) = recorder.borrow_mut().take() {
        let sample_rate = rec.sample_rate;
        let channels = rec.channels;
        let samples = rec.stop_and_take();
        let duration_s = samples.len() as f32 / (sample_rate as f32 * channels as f32);
        eprintln!(
            "[audio] stopped — {} samples, {:.2}s, {}Hz {}ch",
            samples.len(), duration_s, sample_rate, channels
        );
        let mono = audio::to_mono(&samples, channels);
        let wav = audio::to_wav_bytes(&mono, sample_rate, 1);
        eprintln!("[audio] WAV size: {} bytes", wav.len());

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

        spawn_segment_poll(seg_rx, panel, cfg, history);
    }
}

// ── timer helpers ─────────────────────────────────────────────────────────

fn spawn_streaming_timer(
    recorder: Rc<RefCell<Option<audio::Recorder>>>,
    recording: Rc<RefCell<bool>>,
    panel: Rc<DictationPanel>,
    port: u16,
) {
    let stream_rx: Rc<RefCell<Option<StreamRx>>> = Rc::new(RefCell::new(None));
    let stream_last_len: Rc<RefCell<usize>> = Rc::new(RefCell::new(0));

    glib::timeout_add_local(std::time::Duration::from_millis(200), move || {
        if !*recording.borrow() {
            return glib::ControlFlow::Break;
        }

        let got: Option<String> = {
            let mut rx_opt = stream_rx.borrow_mut();
            if let Some(ref rx) = *rx_opt {
                use crossbeam_channel::TryRecvError;
                match rx.try_recv() {
                    Ok(r) => { *rx_opt = None; r }
                    Err(TryRecvError::Disconnected) => { *rx_opt = None; None }
                    Err(TryRecvError::Empty) => None,
                }
            } else {
                None
            }
        };
        if let Some(text) = got {
            if *recording.borrow() {
                panel.set_transcript(&text);
            }
        }

        if stream_rx.borrow().is_none() {
            let maybe_wav = {
                let rec_opt = recorder.borrow();
                rec_opt.as_ref().and_then(|rec| {
                    let count = rec.sample_count();
                    let min_new = (rec.sample_rate as usize) * (rec.channels as usize) * 3;
                    if count >= *stream_last_len.borrow() + min_new {
                        Some(rec.peek_samples())
                    } else {
                        None
                    }
                })
            };
            if let Some(captured) = maybe_wav {
                *stream_last_len.borrow_mut() = captured.samples.len();
                let mono = audio::to_mono(&captured.samples, captured.channels);
                let wav = audio::to_wav_bytes(&mono, captured.sample_rate, 1);
                let (tx, rx) = bounded::<Option<String>>(1);
                *stream_rx.borrow_mut() = Some(rx);
                std::thread::spawn(move || {
                    let rt = tokio::runtime::Runtime::new().unwrap();
                    let client = WhisperClient::new(port);
                    let result = rt.block_on(client.transcribe(&wav, |_| {})).ok();
                    let _ = tx.send(result);
                });
            }
        }

        glib::ControlFlow::Continue
    });
}

fn spawn_segment_poll(
    seg_rx: crossbeam_channel::Receiver<Result<String, String>>,
    panel: Rc<DictationPanel>,
    cfg: Config,
    history: Rc<RefCell<History>>,
) {
    let full_text = Rc::new(RefCell::new(String::new()));
    let first_seg = Rc::new(RefCell::new(true));

    glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
        loop {
            match seg_rx.try_recv() {
                Ok(Ok(seg)) => {
                    if *first_seg.borrow() {
                        *first_seg.borrow_mut() = false;
                        panel.set_transcript(&seg);
                        *full_text.borrow_mut() = seg;
                    } else {
                        panel.append_segment(&seg);
                        let to_push = ui::dictation_panel::space_join(&full_text.borrow(), &seg);
                        full_text.borrow_mut().push_str(&to_push);
                    }
                }
                Ok(Err(e)) => {
                    eprintln!("[whisper] {e}");
                    panel.finish("", &cfg, Rc::clone(&history));
                    return glib::ControlFlow::Break;
                }
                Err(crossbeam_channel::TryRecvError::Empty) => {
                    return glib::ControlFlow::Continue;
                }
                Err(crossbeam_channel::TryRecvError::Disconnected) => {
                    let text = panel.text_view_text();
                    panel.finish(&text, &cfg, Rc::clone(&history));
                    return glib::ControlFlow::Break;
                }
            }
        }
    });
}

// ── utilities ─────────────────────────────────────────────────────────────

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
    let w = win.clone();
    glib::timeout_add_local_once(std::time::Duration::from_millis(50), move || {
        system::x11_window::center_window_on_cursor_monitor(&w);
    });
}
