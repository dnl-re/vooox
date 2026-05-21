//! First-run setup wizard.
//!
//! Three pages, navigated linearly:
//!   1. System check — `python3` ≥ 3.10 and `python3 -m venv` available.
//!   2. Create venv + pip install faster-whisper websockets, with live log.
//!   3. Choose initial model + explicit "download now" or "later" button.
//!
//! On completion the marker file (`paths::setup_marker()`) is written and the
//! supplied `on_done` callback runs — that bootstraps the normal app.

use crate::paths;
use crate::sidecar;
use crate::whisper_client::WhisperClient;
use crate::whisper_models;
use glib;
use gtk4::prelude::*;
use gtk4::{
    Application, ApplicationWindow, Box as GtkBox, Button, ComboBoxText, Label, Orientation,
    ScrolledWindow, Spinner, Stack, StackTransitionType, TextBuffer, TextView,
};
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::sync::mpsc;

pub fn show<F: Fn() + 'static>(app: &Application, on_done: F) {
    let window = ApplicationWindow::builder()
        .application(app)
        .title("vooox — Einrichtung")
        .default_width(680)
        .default_height(540)
        .build();

    let stack = Stack::new();
    stack.set_transition_type(StackTransitionType::SlideLeftRight);

    let on_done: Rc<dyn Fn()> = Rc::new(on_done);

    stack.add_named(&build_page_system_check(&stack), Some("check"));
    stack.add_named(&build_page_install(&stack), Some("install"));
    stack.add_named(
        &build_page_model(&window, &stack, Rc::clone(&on_done)),
        Some("model"),
    );

    let vbox = GtkBox::new(Orientation::Vertical, 0);
    vbox.append(&stack);
    window.set_child(Some(&vbox));

    window.present();
    let w = window.clone();
    glib::timeout_add_local_once(std::time::Duration::from_millis(50), move || {
        crate::x11_window::center_window_on_cursor_monitor(&w);
    });

    // If venv already exists when entering the wizard (e.g. partial setup,
    // missing marker file only), skip ahead to the model page so we don't make
    // the user reinstall.
    if paths::venv_has_faster_whisper() {
        stack.set_visible_child_name("model");
    }
}

// ── page 1: system check ──────────────────────────────────────────────────

fn build_page_system_check(stack: &Stack) -> GtkBox {
    let page = page_box();

    let title = heading("1 · System prüfen");
    let status = Label::new(Some("Prüfe Python-Installation…"));
    status.set_xalign(0.0);
    status.set_wrap(true);

    let instructions = Label::new(None);
    instructions.set_xalign(0.0);
    instructions.set_wrap(true);
    instructions.set_selectable(true);

    let recheck_btn = Button::with_label("Erneut prüfen");
    let next_btn = Button::with_label("Weiter");
    next_btn.set_sensitive(false);

    let buttons = button_row(&[&recheck_btn, &next_btn]);

    page.append(&title);
    page.append(&status);
    page.append(&instructions);
    page.append(&buttons);

    let run_check = {
        let status = status.clone();
        let instructions = instructions.clone();
        let next_btn = next_btn.clone();
        move || {
            match check_python() {
                Ok(version) => {
                    status.set_text(&format!("✓ Python {version} mit venv-Modul gefunden."));
                    instructions.set_text("");
                    next_btn.set_sensitive(true);
                }
                Err(reason) => {
                    status.set_text(&format!("✗ {reason}"));
                    instructions.set_markup(&install_instructions_markup());
                    next_btn.set_sensitive(false);
                }
            }
        }
    };
    run_check();

    {
        let run_check = run_check.clone();
        recheck_btn.connect_clicked(move |_| run_check());
    }
    {
        let stack = stack.clone();
        next_btn.connect_clicked(move |_| {
            stack.set_visible_child_name("install");
        });
    }

    page
}

fn check_python() -> Result<String, String> {
    let out = Command::new("python3")
        .arg("--version")
        .output()
        .map_err(|_| "python3 nicht gefunden".to_string())?;
    if !out.status.success() {
        return Err("python3 nicht ausführbar".into());
    }
    let version_line = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let version_line = if version_line.is_empty() {
        String::from_utf8_lossy(&out.stderr).trim().to_string()
    } else {
        version_line
    };
    // expect "Python 3.X.Y"
    let version = version_line
        .strip_prefix("Python ")
        .ok_or_else(|| format!("unerwartete Versionsausgabe: {version_line}"))?
        .to_string();
    let (major, minor) = parse_major_minor(&version)
        .ok_or_else(|| format!("Version nicht parsbar: {version}"))?;
    if major < 3 || (major == 3 && minor < 10) {
        return Err(format!(
            "Python {version} ist zu alt — benötigt wird mindestens 3.10."
        ));
    }
    // also verify venv module is present
    let venv_check = Command::new("python3")
        .args(["-m", "venv", "--help"])
        .output()
        .map_err(|e| format!("python3 -m venv: {e}"))?;
    if !venv_check.status.success() {
        return Err("python3 ist da, aber das venv-Modul fehlt.".into());
    }
    Ok(version)
}

fn parse_major_minor(s: &str) -> Option<(u32, u32)> {
    let mut parts = s.split('.');
    let major: u32 = parts.next()?.parse().ok()?;
    let minor: u32 = parts.next()?.parse().ok()?;
    Some((major, minor))
}

fn install_instructions_markup() -> String {
    // Best-effort distro hint via /etc/os-release; falls back to all-three.
    let id = std::fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("ID="))
                .map(|l| l.trim_start_matches("ID=").trim_matches('"').to_string())
        })
        .unwrap_or_default();
    let hint = match id.as_str() {
        "debian" | "ubuntu" | "linuxmint" | "pop" => {
            "sudo apt install python3 python3-venv"
        }
        "fedora" | "rhel" | "centos" => "sudo dnf install python3",
        "arch" | "manjaro" | "endeavouros" => "sudo pacman -S python",
        _ => {
            return "<b>Installiere Python 3.10+ inklusive venv-Modul.</b>\n\
                 Debian/Ubuntu: <tt>sudo apt install python3 python3-venv</tt>\n\
                 Fedora: <tt>sudo dnf install python3</tt>\n\
                 Arch: <tt>sudo pacman -S python</tt>"
                .into()
        }
    };
    format!(
        "<b>So installierst du es:</b>\n<tt>{}</tt>\n\nDanach auf \"Erneut prüfen\" klicken.",
        hint
    )
}

// ── page 2: install venv ──────────────────────────────────────────────────

fn build_page_install(stack: &Stack) -> GtkBox {
    let page = page_box();

    let title = heading("2 · Python-Umgebung einrichten");
    let info = Label::new(Some(&format!(
        "Es wird eine isolierte Python-Umgebung unter\n{}\nangelegt und faster-whisper installiert \
         (ca. 600 MB Download, dauert ein paar Minuten).",
        paths::venv_dir().display()
    )));
    info.set_xalign(0.0);
    info.set_wrap(true);

    let start_btn = Button::with_label("Einrichtung starten");
    let next_btn = Button::with_label("Weiter");
    next_btn.set_sensitive(false);
    let buttons = button_row(&[&start_btn, &next_btn]);

    let spinner = Spinner::new();
    spinner.set_visible(false);

    let log_buffer = TextBuffer::new(None);
    let log_view = TextView::with_buffer(&log_buffer);
    log_view.set_editable(false);
    log_view.set_monospace(true);
    let log_scroll = ScrolledWindow::builder()
        .vexpand(true)
        .child(&log_view)
        .build();

    page.append(&title);
    page.append(&info);
    page.append(&spinner);
    page.append(&log_scroll);
    page.append(&buttons);

    {
        let start_btn = start_btn.clone();
        let next_btn = next_btn.clone();
        let spinner = spinner.clone();
        let log_buffer = log_buffer.clone();
        start_btn.connect_clicked(move |btn| {
            btn.set_sensitive(false);
            spinner.set_visible(true);
            spinner.start();
            log_buffer.set_text("");
            let (tx, rx) = mpsc::channel::<InstallMsg>();
            spawn_install_thread(tx);

            let log_buffer = log_buffer.clone();
            let spinner = spinner.clone();
            let next_btn = next_btn.clone();
            let start_btn = btn.clone();
            glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
                let mut done = false;
                let mut had_error = false;
                while let Ok(m) = rx.try_recv() {
                    match m {
                        InstallMsg::Line(line) => append_log(&log_buffer, &line),
                        InstallMsg::Done => {
                            done = true;
                            break;
                        }
                        InstallMsg::Error(e) => {
                            append_log(&log_buffer, &format!("\n✗ FEHLER: {e}"));
                            had_error = true;
                            done = true;
                            break;
                        }
                    }
                }
                if done {
                    spinner.stop();
                    spinner.set_visible(false);
                    if had_error {
                        start_btn.set_label("Erneut versuchen");
                        start_btn.set_sensitive(true);
                    } else {
                        append_log(&log_buffer, "\n✓ Einrichtung abgeschlossen.");
                        next_btn.set_sensitive(true);
                    }
                    glib::ControlFlow::Break
                } else {
                    glib::ControlFlow::Continue
                }
            });
        });
    }
    {
        let stack = stack.clone();
        next_btn.connect_clicked(move |_| {
            stack.set_visible_child_name("model");
        });
    }

    page
}

enum InstallMsg {
    Line(String),
    Done,
    Error(String),
}

fn spawn_install_thread(tx: mpsc::Sender<InstallMsg>) {
    std::thread::spawn(move || {
        let venv = paths::venv_dir();
        if let Some(parent) = venv.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                let _ = tx.send(InstallMsg::Error(format!("mkdir {}: {e}", parent.display())));
                return;
            }
        }

        let steps: &[(&str, Vec<String>)] = &[
            (
                "venv anlegen",
                vec!["python3".into(), "-m".into(), "venv".into(), venv.display().to_string()],
            ),
            (
                "pip aktualisieren",
                vec![
                    venv.join("bin/pip").display().to_string(),
                    "install".into(),
                    "--upgrade".into(),
                    "pip".into(),
                ],
            ),
            (
                "faster-whisper installieren",
                vec![
                    venv.join("bin/pip").display().to_string(),
                    "install".into(),
                    "faster-whisper".into(),
                    "websockets".into(),
                ],
            ),
        ];

        for (label, cmd) in steps {
            let _ = tx.send(InstallMsg::Line(format!("$ # {label}")));
            let _ = tx.send(InstallMsg::Line(format!("$ {}", cmd.join(" "))));
            let (prog, args) = cmd.split_first().unwrap();
            let mut child = match Command::new(prog)
                .args(args)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
            {
                Ok(c) => c,
                Err(e) => {
                    let _ = tx.send(InstallMsg::Error(format!("{label}: {e}")));
                    return;
                }
            };
            // stream both stdout and stderr
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
                    let _ = tx.send(InstallMsg::Error(format!("{label}: {e}")));
                    return;
                }
            };
            let _ = h_out.join();
            let _ = h_err.join();
            if !status.success() {
                let _ = tx.send(InstallMsg::Error(format!(
                    "{label}: exit {}",
                    status.code().unwrap_or(-1)
                )));
                return;
            }
        }
        let _ = tx.send(InstallMsg::Done);
    });
}

fn append_log(buf: &TextBuffer, line: &str) {
    let mut iter = buf.end_iter();
    if buf.char_count() > 0 {
        buf.insert(&mut iter, "\n");
    }
    buf.insert(&mut iter, line);
}

// ── page 3: model download ───────────────────────────────────────────────

fn build_page_model(
    window: &ApplicationWindow,
    _stack: &Stack,
    on_done: Rc<dyn Fn()>,
) -> GtkBox {
    let page = page_box();

    let title = heading("3 · Sprachmodell wählen");
    let info = Label::new(Some(
        "Wähle ein Whisper-Modell. Größere Modelle erkennen besser, brauchen aber \
         mehr Plattenplatz und Rechenzeit. Du kannst das später in den Einstellungen \
         jederzeit ändern.",
    ));
    info.set_xalign(0.0);
    info.set_wrap(true);

    let combo = ComboBoxText::new();
    for m in whisper_models::MODELS {
        combo.append(Some(m.id), &format!("{} ({})", m.id, whisper_models::size_label(m.id)));
    }
    combo.set_active_id(Some("small"));

    let size_lbl = Label::new(None);
    size_lbl.set_xalign(0.0);
    size_lbl.set_wrap(true);
    {
        let size_lbl = size_lbl.clone();
        let update = move |id: &str| {
            size_lbl.set_markup(&format!(
                "<b>Lädt {} herunter.</b>",
                whisper_models::size_label(id)
            ));
        };
        update("small");
        combo.connect_changed(move |cb| {
            if let Some(id) = cb.active_id() {
                update(&id);
            }
        });
    }

    let download_btn = Button::with_label("Modell jetzt herunterladen");
    let later_btn = Button::with_label("Später (vorerst kein Download)");
    let buttons = button_row(&[&download_btn, &later_btn]);

    let spinner = Spinner::new();
    spinner.set_visible(false);
    let status = Label::new(None);
    status.set_xalign(0.0);
    status.set_wrap(true);

    page.append(&title);
    page.append(&info);
    page.append(&combo);
    page.append(&size_lbl);
    page.append(&spinner);
    page.append(&status);
    page.append(&buttons);

    let window = window.clone();

    {
        let combo = combo.clone();
        let spinner = spinner.clone();
        let status = status.clone();
        let download_btn = download_btn.clone();
        let later_btn = later_btn.clone();
        let on_done = Rc::clone(&on_done);
        let window = window.clone();
        download_btn.connect_clicked(move |btn| {
            let Some(model) = combo.active_id() else { return };
            let model = model.to_string();
            btn.set_sensitive(false);
            later_btn.set_sensitive(false);
            spinner.set_visible(true);
            spinner.start();
            status.set_text(&format!(
                "Lade {model} herunter — das kann eine Weile dauern. Bitte nicht schließen."
            ));

            let (tx, rx) = mpsc::channel::<Result<(), String>>();
            run_ensure_model(model.clone(), tx);

            let spinner = spinner.clone();
            let status = status.clone();
            let btn = btn.clone();
            let later_btn = later_btn.clone();
            let on_done = Rc::clone(&on_done);
            let window = window.clone();
            glib::timeout_add_local(std::time::Duration::from_millis(200), move || {
                match rx.try_recv() {
                    Ok(Ok(())) => {
                        spinner.stop();
                        spinner.set_visible(false);
                        status.set_markup("<b>✓ Modell bereit.</b>");
                        let _ = paths::mark_setup_complete();
                        show_ready_page(&window, Rc::clone(&on_done));
                        glib::ControlFlow::Break
                    }
                    Ok(Err(e)) => {
                        spinner.stop();
                        spinner.set_visible(false);
                        status.set_text(&format!("✗ Fehler: {e}"));
                        btn.set_sensitive(true);
                        later_btn.set_sensitive(true);
                        glib::ControlFlow::Break
                    }
                    Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        spinner.stop();
                        spinner.set_visible(false);
                        status.set_text("✗ Download-Thread abgebrochen.");
                        btn.set_sensitive(true);
                        later_btn.set_sensitive(true);
                        glib::ControlFlow::Break
                    }
                }
            });
        });
    }

    {
        let on_done = Rc::clone(&on_done);
        let window = window.clone();
        later_btn.connect_clicked(move |_| {
            let _ = paths::mark_setup_complete();
            show_ready_page(&window, Rc::clone(&on_done));
        });
    }

    page
}

/// Replaces the wizard contents with a final confirmation page. Without this
/// the window would close instantly after a (cached) ensure_model call, and
/// the user would have no clear signal that the app is now starting up.
fn show_ready_page(window: &ApplicationWindow, on_done: Rc<dyn Fn()>) {
    let page = page_box();
    let title = heading("✓ Einrichtung abgeschlossen");
    let info = Label::new(None);
    info.set_markup(&format!(
        "vooox ist einsatzbereit. Klicke auf \"App starten\", um das Hauptfenster \
         zu öffnen.\n\nDer globale Shortcut ist standardmäßig <tt>{}</tt> — kurz drücken \
         startet/stoppt die Aufnahme, lange halten aktiviert Push-to-Talk. \
         Du kannst ihn in den Einstellungen ändern.",
        crate::config::Config::default().shortcut,
    ));
    info.set_xalign(0.0);
    info.set_wrap(true);

    let start_btn = Button::with_label("App starten");
    start_btn.add_css_class("suggested-action");
    let buttons = button_row(&[&start_btn]);

    page.append(&title);
    page.append(&info);
    page.append(&buttons);

    window.set_child(Some(&page));

    let window = window.clone();
    start_btn.connect_clicked(move |_| {
        window.close();
        on_done();
    });
}

/// Spawns the sidecar (using the freshly-installed venv), sends the
/// ensure_model request, then kills the sidecar. The setup wizard doesn't
/// keep a long-lived sidecar — the normal app bootstrap will spawn its own.
fn run_ensure_model(model: String, tx: mpsc::Sender<Result<(), String>>) {
    std::thread::spawn(move || {
        let (mut child, port) = match sidecar::spawn_sidecar() {
            Ok(x) => x,
            Err(e) => {
                let _ = tx.send(Err(format!("Sidecar-Start fehlgeschlagen: {e}")));
                return;
            }
        };
        let rt = match tokio::runtime::Runtime::new() {
            Ok(r) => r,
            Err(e) => {
                let _ = child.kill();
                let _ = tx.send(Err(format!("Tokio-Runtime: {e}")));
                return;
            }
        };
        let result = rt.block_on(async {
            crate::whisper_client::wait_for_ready(port, 60).await?;
            let client = WhisperClient::new(port);
            client.ensure_model(&model).await
        });
        let _ = child.kill();
        let _ = tx.send(result);
    });
}

// ── helpers ──────────────────────────────────────────────────────────────

fn page_box() -> GtkBox {
    let b = GtkBox::new(Orientation::Vertical, 12);
    b.set_margin_top(16);
    b.set_margin_bottom(16);
    b.set_margin_start(20);
    b.set_margin_end(20);
    b
}

fn heading(text: &str) -> Label {
    let l = Label::new(None);
    l.set_markup(&format!("<span size='x-large' weight='bold'>{text}</span>"));
    l.set_xalign(0.0);
    l
}

fn button_row(buttons: &[&Button]) -> GtkBox {
    let row = GtkBox::new(Orientation::Horizontal, 8);
    row.set_halign(gtk4::Align::End);
    for b in buttons {
        row.append(*b);
    }
    row
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_parses() {
        assert_eq!(parse_major_minor("3.11.6"), Some((3, 11)));
        assert_eq!(parse_major_minor("3.10"), Some((3, 10)));
        assert_eq!(parse_major_minor("garbage"), None);
    }
}

