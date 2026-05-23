//! First-run setup wizard.
//!
//! Three pages, navigated linearly:
//!   1. System check — `python3` ≥ 3.10 and `python3 -m venv` available.
//!   2. Create venv + pip install faster-whisper websockets, with live log.
//!   3. Choose initial model + explicit "download now" or "later" button.
//!
//! On completion the marker file (`paths::setup_marker()`) is written and the
//! supplied `on_done` callback runs — that bootstraps the normal app.

use crate::system::gpu;
use crate::storage::paths;
use crate::transcription::sidecar;
use crate::transcription::whisper_client::WhisperClient;
use crate::transcription::whisper_models;
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
        .default_width(740)
        .default_height(560)
        .build();

    let stack = Stack::new();
    stack.set_transition_type(StackTransitionType::SlideLeftRight);

    let on_done: Rc<dyn Fn()> = Rc::new(on_done);

    stack.add_named(&build_page_system_check(&stack), Some("check"));
    stack.add_named(&build_page_install(&stack), Some("install"));
    stack.add_named(&build_page_gpu(&stack), Some("gpu"));
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
        crate::system::x11_window::center_window_on_cursor_monitor(&w);
    });

    if paths::venv_has_faster_whisper() {
        stack.set_visible_child_name("gpu");
    }
}

// ── page 1: system check ──────────────────────────────────────────────────

fn build_page_system_check(stack: &Stack) -> GtkBox {
    let page = page_box();

    let title = heading("1 · System prüfen");
    let subtitle = Label::new(Some(
        "vooox prüft, welche Voraussetzungen schon erfüllt sind.",
    ));
    subtitle.set_xalign(0.0);
    subtitle.add_css_class("dim-label");
    subtitle.set_margin_bottom(6);

    let python_row = StatusRow::new();
    let xdotool_row = StatusRow::new();
    let ydotool_row = StatusRow::new();

    let list = GtkBox::new(Orientation::Vertical, 8);
    list.append(&python_row.widget);
    list.append(&xdotool_row.widget);
    list.append(&ydotool_row.widget);

    let instructions = Label::new(None);
    instructions.set_xalign(0.0);
    instructions.set_wrap(true);
    instructions.set_selectable(true);
    instructions.set_margin_top(8);

    let spacer = GtkBox::new(Orientation::Vertical, 0);
    spacer.set_vexpand(true);

    let recheck_btn = Button::with_label("Erneut prüfen");
    let next_btn = Button::with_label("Weiter");
    next_btn.add_css_class("suggested-action");
    next_btn.set_sensitive(false);

    let buttons = button_row(&[&recheck_btn, &next_btn]);

    page.append(&title);
    page.append(&subtitle);
    page.append(&list);
    page.append(&instructions);
    page.append(&spacer);
    page.append(&buttons);

    let run_check = {
        let python_row = python_row.clone();
        let xdotool_row = xdotool_row.clone();
        let ydotool_row = ydotool_row.clone();
        let instructions = instructions.clone();
        let next_btn = next_btn.clone();
        move || {
            let python_ok = match check_python() {
                Ok(version) => {
                    python_row.set(
                        Status::Ok,
                        &format!("Python <b>{version}</b> mit <tt>venv</tt>-Modul gefunden."),
                    );
                    true
                }
                Err(reason) => {
                    python_row.set(Status::Error, &format!("<b>Python:</b> {reason}"));
                    false
                }
            };
            if which("xdotool") {
                xdotool_row.set(
                    Status::Ok,
                    "<tt>xdotool</tt> gefunden — Auto-Paste &amp; Fenster-Positionierung verfügbar.",
                );
            } else {
                xdotool_row.set(
                    Status::Warn,
                    "<tt>xdotool</tt> nicht gefunden — Auto-Paste &amp; Fenster-Positionierung deaktiviert.",
                );
            }
            if which("ydotool") {
                ydotool_row.set(
                    Status::Ok,
                    "<tt>ydotool</tt> gefunden — Text-Injection unter Wayland möglich.",
                );
            } else {
                ydotool_row.set(
                    Status::Info,
                    "<tt>ydotool</tt> nicht gefunden — nur relevant für native Wayland-Apps.",
                );
            }

            if python_ok {
                instructions.set_text("");
                next_btn.set_sensitive(true);
            } else {
                instructions.set_markup(&install_instructions_markup());
                next_btn.set_sensitive(false);
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
    let venv_check = Command::new("python3")
        .args(["-m", "venv", "--help"])
        .output()
        .map_err(|e| format!("python3 -m venv: {e}"))?;
    if !venv_check.status.success() {
        return Err("python3 ist da, aber das venv-Modul fehlt.".into());
    }
    Ok(version)
}

#[derive(Clone, Copy)]
enum Status {
    Ok,
    Warn,
    Info,
    Error,
}

impl Status {
    fn icon(self) -> &'static str {
        match self {
            Status::Ok => "✓",
            Status::Warn => "⚠",
            Status::Info => "ℹ",
            Status::Error => "✗",
        }
    }
    fn color(self) -> &'static str {
        match self {
            Status::Ok => "#26a269",
            Status::Warn => "#e5a50a",
            Status::Info => "#9a9996",
            Status::Error => "#c01c28",
        }
    }
}

#[derive(Clone)]
struct StatusRow {
    widget: GtkBox,
    icon: Label,
    text: Label,
}

impl StatusRow {
    fn new() -> Self {
        let widget = GtkBox::new(Orientation::Horizontal, 10);
        widget.set_valign(gtk4::Align::Start);

        let icon = Label::new(None);
        icon.set_xalign(0.5);
        icon.set_valign(gtk4::Align::Start);
        icon.set_width_chars(2);

        let text = Label::new(Some("…"));
        text.set_xalign(0.0);
        text.set_wrap(true);
        text.set_hexpand(true);
        text.set_use_markup(true);

        widget.append(&icon);
        widget.append(&text);
        StatusRow { widget, icon, text }
    }

    fn set(&self, status: Status, markup_text: &str) {
        self.icon.set_markup(&format!(
            "<span foreground='{}' weight='bold' size='large'>{}</span>",
            status.color(),
            status.icon()
        ));
        self.text.set_markup(markup_text);
    }
}

fn which(cmd: &str) -> bool {
    Command::new("sh")
        .args(["-c", &format!("command -v {cmd} >/dev/null 2>&1")])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn parse_major_minor(s: &str) -> Option<(u32, u32)> {
    let mut parts = s.split('.');
    let major: u32 = parts.next()?.parse().ok()?;
    let minor: u32 = parts.next()?.parse().ok()?;
    Some((major, minor))
}

fn install_instructions_markup() -> String {
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
    next_btn.add_css_class("suggested-action");
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
        let log_view = log_view.clone();
        start_btn.connect_clicked(move |btn| {
            btn.set_sensitive(false);
            spinner.set_visible(true);
            spinner.start();
            log_buffer.set_text("");
            let (tx, rx) = mpsc::channel::<InstallMsg>();
            spawn_install_thread(tx);

            let log_buffer = log_buffer.clone();
            let log_view = log_view.clone();
            let spinner = spinner.clone();
            let next_btn = next_btn.clone();
            let start_btn = btn.clone();
            glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
                let mut done = false;
                let mut had_error = false;
                while let Ok(m) = rx.try_recv() {
                    match m {
                        InstallMsg::Line(line) => append_log(&log_buffer, &log_view, &line),
                        InstallMsg::Done => {
                            done = true;
                            break;
                        }
                        InstallMsg::Error(e) => {
                            append_log(&log_buffer, &log_view, &format!("\n✗ FEHLER: {e}"));
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
                        append_log(&log_buffer, &log_view, "");
                        append_log(
                            &log_buffer,
                            &log_view,
                            "✓ Successfully installed vooox dependencies.",
                        );
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
            stack.set_visible_child_name("gpu");
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

fn append_log(buf: &TextBuffer, view: &TextView, line: &str) {
    let mut iter = buf.end_iter();
    if buf.char_count() > 0 {
        buf.insert(&mut iter, "\n");
    }
    let line_start = buf.create_mark(None, &iter, true);
    buf.insert(&mut iter, line);
    view.scroll_to_mark(&line_start, 0.0, true, 0.0, 1.0);
    buf.delete_mark(&line_start);
}

// ── page 3: GPU-Beschleunigung ───────────────────────────────────────────

fn build_page_gpu(stack: &Stack) -> GtkBox {
    let page = page_box();

    let title = heading("3 · GPU-Beschleunigung");
    let subtitle = Label::new(Some(
        "Whisper kann optional auf einer NVIDIA-GPU laufen — das macht vor allem bei \
         den größeren Modellen (medium, large-v3) einen großen Geschwindigkeitsunterschied. \
         Ohne GPU sind die kleineren Modelle (tiny, small) die sinnvolle Wahl.",
    ));
    subtitle.set_xalign(0.0);
    subtitle.set_wrap(true);

    let status_row = StatusRow::new();
    let detail_lbl = Label::new(None);
    detail_lbl.set_xalign(0.0);
    detail_lbl.set_wrap(true);
    detail_lbl.set_use_markup(true);

    let install_btn = Button::with_label(&format!(
        "GPU-Unterstützung installieren ({})",
        gpu::estimated_download_label()
    ));
    install_btn.add_css_class("suggested-action");
    install_btn.set_visible(false);

    let spinner = Spinner::new();
    spinner.set_visible(false);

    let spinner_row = GtkBox::new(Orientation::Horizontal, 8);
    spinner_row.set_halign(gtk4::Align::Start);
    spinner_row.append(&spinner);

    let log_buffer = TextBuffer::new(None);
    let log_view = TextView::with_buffer(&log_buffer);
    log_view.set_editable(false);
    log_view.set_monospace(true);
    let log_scroll = ScrolledWindow::builder()
        .height_request(180)
        .child(&log_view)
        .build();
    log_scroll.set_visible(false);

    let spacer = GtkBox::new(Orientation::Vertical, 0);
    spacer.set_vexpand(true);

    let skip_btn = Button::with_label("Überspringen");
    let next_btn = Button::with_label("Weiter");
    next_btn.add_css_class("suggested-action");
    let buttons = button_row(&[&skip_btn, &install_btn, &next_btn]);

    page.append(&title);
    page.append(&subtitle);
    page.append(&status_row.widget);
    page.append(&detail_lbl);
    page.append(&spinner_row);
    page.append(&log_scroll);
    page.append(&spacer);
    page.append(&buttons);

    let refresh = {
        let status_row = status_row.clone();
        let detail_lbl = detail_lbl.clone();
        let install_btn = install_btn.clone();
        let next_btn = next_btn.clone();
        let skip_btn = skip_btn.clone();
        move || {
            let hw = gpu::detect_hardware();
            let wheels = gpu::wheels_installed();
            match (&hw, wheels) {
                (gpu::NvidiaHardware::None, _) => {
                    status_row.set(
                        Status::Info,
                        "Keine NVIDIA-GPU im System gefunden — vooox läuft auf der CPU.",
                    );
                    detail_lbl.set_markup(
                        "<b>Empfehlung:</b> Wähle auf der nächsten Seite ein kleines Modell \
                         (<tt>tiny</tt> oder <tt>small</tt>). Größere Modelle wären auf der CPU \
                         deutlich zu langsam für flüssiges Diktieren.",
                    );
                    install_btn.set_visible(false);
                    skip_btn.set_label("Weiter");
                    skip_btn.set_visible(false);
                    next_btn.set_visible(true);
                }
                (gpu::NvidiaHardware::NoDriver, _) => {
                    status_row.set(
                        Status::Warn,
                        "NVIDIA-Karte erkannt, aber der proprietäre Treiber ist nicht installiert.",
                    );
                    detail_lbl.set_markup(
                        "Installiere den Treiber (z. B. <tt>nvidia-driver-535</tt>) über deinen \
                         Paketmanager und starte den Rechner neu, um GPU-Beschleunigung später \
                         zu aktivieren.\n\n\
                         <b>Empfehlung für jetzt:</b> Wähle ein kleines Modell \
                         (<tt>tiny</tt> oder <tt>small</tt>) — vooox läuft solange auf der CPU.",
                    );
                    install_btn.set_visible(false);
                    skip_btn.set_visible(false);
                    next_btn.set_visible(true);
                }
                (gpu::NvidiaHardware::DriverTooOld { driver }, _) => {
                    status_row.set(
                        Status::Warn,
                        &format!(
                            "NVIDIA-Treiber <b>{driver}</b> ist zu alt — CUDA 12 braucht \
                             mindestens Version 525.",
                        ),
                    );
                    detail_lbl.set_markup(
                        "Aktualisiere den Treiber über deinen Paketmanager, um GPU-Beschleunigung \
                         später nutzen zu können.\n\n\
                         <b>Empfehlung für jetzt:</b> Wähle ein kleines Modell \
                         (<tt>tiny</tt> oder <tt>small</tt>).",
                    );
                    install_btn.set_visible(false);
                    skip_btn.set_visible(false);
                    next_btn.set_visible(true);
                }
                (gpu::NvidiaHardware::Ok { driver }, false) => {
                    status_row.set(
                        Status::Ok,
                        &format!(
                            "NVIDIA-GPU erkannt — Treiber <b>{driver}</b> unterstützt CUDA 12.",
                        ),
                    );
                    detail_lbl.set_markup(
                        "Du kannst die CUDA- und cuDNN-Libraries jetzt installieren — das \
                         beschleunigt die Transkription erheblich und macht die Modelle \
                         <tt>medium</tt> und <tt>large-v3</tt> erst sinnvoll nutzbar. \
                         Du kannst das auch später unter <tt>Einstellungen → Whisper</tt> \
                         nachholen.",
                    );
                    install_btn.set_visible(true);
                    install_btn.set_sensitive(true);
                    skip_btn.set_visible(true);
                    next_btn.set_visible(false);
                }
                (gpu::NvidiaHardware::Ok { driver }, true) => {
                    status_row.set(
                        Status::Ok,
                        &format!(
                            "GPU-Beschleunigung eingerichtet — Treiber <b>{driver}</b>, \
                             CUDA-Libraries installiert.",
                        ),
                    );
                    detail_lbl.set_markup(
                        "Du kannst alle Modelle inklusive <tt>medium</tt> und <tt>large-v3</tt> \
                         flüssig nutzen.",
                    );
                    install_btn.set_visible(false);
                    skip_btn.set_visible(false);
                    next_btn.set_visible(true);
                }
            }
        }
    };
    refresh();

    {
        let spinner = spinner.clone();
        let log_buffer = log_buffer.clone();
        let log_view = log_view.clone();
        let log_scroll = log_scroll.clone();
        let skip_btn = skip_btn.clone();
        let refresh = refresh.clone();
        install_btn.connect_clicked(move |btn| {
            btn.set_sensitive(false);
            skip_btn.set_sensitive(false);
            spinner.set_visible(true);
            spinner.start();
            log_scroll.set_visible(true);
            log_buffer.set_text("");

            let (tx, rx) = mpsc::channel::<gpu::InstallMsg>();
            gpu::spawn_cuda_install_thread(tx);

            let spinner = spinner.clone();
            let log_buffer = log_buffer.clone();
            let log_view = log_view.clone();
            let skip_btn = skip_btn.clone();
            let refresh = refresh.clone();
            let btn = btn.clone();
            glib::timeout_add_local(std::time::Duration::from_millis(120), move || {
                let mut done = false;
                let mut had_error = false;
                while let Ok(m) = rx.try_recv() {
                    match m {
                        gpu::InstallMsg::Line(line) => append_log(&log_buffer, &log_view, &line),
                        gpu::InstallMsg::Done => {
                            done = true;
                            break;
                        }
                        gpu::InstallMsg::Error(e) => {
                            append_log(&log_buffer, &log_view, &format!("\n✗ FEHLER: {e}"));
                            had_error = true;
                            done = true;
                            break;
                        }
                    }
                }
                if done {
                    spinner.stop();
                    spinner.set_visible(false);
                    skip_btn.set_sensitive(true);
                    if had_error {
                        btn.set_label("Erneut versuchen");
                        btn.set_sensitive(true);
                    } else {
                        append_log(&log_buffer, &log_view, "");
                        append_log(
                            &log_buffer,
                            &log_view,
                            "✓ CUDA-Libraries installiert. GPU-Beschleunigung ist aktiv.",
                        );
                        refresh();
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
        skip_btn.connect_clicked(move |_| {
            stack.set_visible_child_name("model");
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

// ── page 4: model download ───────────────────────────────────────────────

fn build_page_model(
    window: &ApplicationWindow,
    _stack: &Stack,
    on_done: Rc<dyn Fn()>,
) -> GtkBox {
    let page = page_box();

    let title = heading("4 · Sprachmodell wählen");
    let info = Label::new(Some(
        "Wähle ein Whisper-Modell. Größere Modelle erkennen besser, brauchen aber \
         mehr Plattenplatz und Rechenzeit. Du kannst das später in den Einstellungen \
         jederzeit ändern.",
    ));
    info.set_xalign(0.0);
    info.set_wrap(true);

    let recommendation = Label::new(None);
    recommendation.set_xalign(0.0);
    recommendation.set_wrap(true);
    recommendation.set_use_markup(true);
    let gpu_active = gpu::wheels_installed()
        && matches!(gpu::detect_hardware(), gpu::NvidiaHardware::Ok { .. });
    if gpu_active {
        recommendation.set_markup(
            "<b>Empfehlung (mit GPU):</b> <tt>medium</tt> oder <tt>large-v3</tt> für beste \
             Genauigkeit, oder <tt>small</tt> für schnellere Latenz.",
        );
    } else {
        recommendation.set_markup(
            "<b>Empfehlung (CPU-only):</b> <tt>tiny</tt> oder <tt>small</tt>. Größere Modelle \
             wären auf der CPU zu langsam für flüssiges Diktieren.",
        );
    }

    let combo = ComboBoxText::new();
    for m in whisper_models::MODELS {
        combo.append(
            Some(m.id),
            &format!("{} — {}", m.id, whisper_models::size_label_short(m.id)),
        );
    }
    let default_model = if gpu_active { "medium" } else { "small" };
    combo.set_active_id(Some(default_model));

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
        update(default_model);
        combo.connect_changed(move |cb| {
            if let Some(id) = cb.active_id() {
                update(&id);
            }
        });
    }

    let download_btn = Button::with_label("Jetzt herunterladen");
    download_btn.add_css_class("suggested-action");
    let later_btn = Button::with_label("Später");
    let buttons = button_row(&[&later_btn, &download_btn]);

    let spinner = Spinner::new();
    spinner.set_visible(false);
    let status = Label::new(None);
    status.set_xalign(0.0);
    status.set_wrap(true);

    let spacer = GtkBox::new(Orientation::Vertical, 0);
    spacer.set_vexpand(true);

    page.append(&title);
    page.append(&info);
    page.append(&recommendation);
    page.append(&combo);
    page.append(&size_lbl);
    page.append(&spinner);
    page.append(&status);
    page.append(&spacer);
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

fn show_ready_page(window: &ApplicationWindow, on_done: Rc<dyn Fn()>) {
    let page = page_box();
    let title = heading("✓ Einrichtung abgeschlossen");
    let info = Label::new(None);
    info.set_markup(&format!(
        "vooox ist einsatzbereit. Klicke auf \"App starten\", um das Hauptfenster \
         zu öffnen.\n\nDer globale Shortcut ist standardmäßig <tt>{}</tt> — kurz drücken \
         startet/stoppt die Aufnahme, lange halten aktiviert Push-to-Talk. \
         Du kannst ihn in den Einstellungen ändern.",
        crate::storage::config::Config::default().shortcut,
    ));
    info.set_xalign(0.0);
    info.set_wrap(true);

    let start_btn = Button::with_label("App starten");
    start_btn.add_css_class("suggested-action");
    let buttons = button_row(&[&start_btn]);

    let spacer = GtkBox::new(Orientation::Vertical, 0);
    spacer.set_vexpand(true);

    page.append(&title);
    page.append(&info);
    page.append(&spacer);
    page.append(&buttons);

    window.set_child(Some(&page));

    let window = window.clone();
    start_btn.connect_clicked(move |_| {
        window.close();
        on_done();
    });
}

fn run_ensure_model(model: String, tx: mpsc::Sender<Result<(), String>>) {
    std::thread::spawn(move || {
        let sidecar_process = match sidecar::start_whisper_sidecar() {
            Ok(x) => x,
            Err(e) => {
                let _ = tx.send(Err(format!("Sidecar-Start fehlgeschlagen: {e}")));
                return;
            }
        };
        let mut child = sidecar_process.child;
        let port = sidecar_process.port;
        let rt = match tokio::runtime::Runtime::new() {
            Ok(r) => r,
            Err(e) => {
                let _ = child.kill();
                let _ = tx.send(Err(format!("Tokio-Runtime: {e}")));
                return;
            }
        };
        let result = rt.block_on(async {
            crate::transcription::whisper_client::wait_for_ready(port, 60).await?;
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
