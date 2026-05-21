use crate::audio;
use crate::config::Config;
use crate::gpu;
use crate::whisper_client::WhisperClient;
use crate::whisper_models;
use crate::x11_window;
use glib;
use gtk4::prelude::*;
use gtk4::{
    Adjustment, Application, ApplicationWindow, Box as GtkBox, Button, CheckButton, ComboBoxText,
    Entry, Label, LevelBar, ListBox, ListBoxRow, Notebook, Orientation, ScrolledWindow, Separator,
    SpinButton, Spinner, TextBuffer, TextView, ToggleButton,
};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;

pub struct SettingsWindow {
    window: ApplicationWindow,
}

impl SettingsWindow {
    pub fn new(app: &Application, config: Rc<RefCell<Config>>, whisper_port: u16) -> Self {
        let window = ApplicationWindow::builder()
            .application(app)
            .title("vooox — Einstellungen")
            .default_width(840)
            .default_height(620)
            .build();

        let notebook = Notebook::new();
        notebook.append_page(
            &build_general_tab(Rc::clone(&config)),
            Some(&Label::new(Some("Allgemein"))),
        );
        notebook.append_page(
            &build_microphone_tab(&window, Rc::clone(&config)),
            Some(&Label::new(Some("Mikrofon"))),
        );
        notebook.append_page(
            &build_whisper_tab(Rc::clone(&config), whisper_port),
            Some(&Label::new(Some("Whisper"))),
        );
        notebook.append_page(
            &build_shortcut_tab(Rc::clone(&config)),
            Some(&Label::new(Some("Tastenkürzel"))),
        );

        let save_btn = Button::with_label("Speichern & schließen");
        {
            let config = Rc::clone(&config);
            let win = window.clone();
            save_btn.connect_clicked(move |_| {
                if let Err(e) = config.borrow().save() {
                    eprintln!("[settings] save error: {e}");
                }
                win.close();
            });
        }

        let vbox = GtkBox::new(Orientation::Vertical, 8);
        vbox.set_margin_top(8);
        vbox.set_margin_bottom(8);
        vbox.set_margin_start(8);
        vbox.set_margin_end(8);
        vbox.append(&notebook);
        vbox.append(&save_btn);
        window.set_child(Some(&vbox));

        SettingsWindow { window }
    }

    pub fn show(&self) {
        self.window.present();
        let win = self.window.clone();
        glib::timeout_add_local_once(std::time::Duration::from_millis(50), move || {
            x11_window::center_window_on_cursor_monitor(&win);
        });
    }
}

// ── Mikrofon-Tab ──────────────────────────────────────────────────────────

fn build_microphone_tab(window: &ApplicationWindow, config: Rc<RefCell<Config>>) -> GtkBox {
    let vbox = GtkBox::new(Orientation::Vertical, 6);
    vbox.set_margin_top(12);
    vbox.set_margin_start(12);
    vbox.set_margin_end(12);

    let devices = audio::list_input_devices();
    if devices.is_empty() {
        vbox.append(&Label::new(Some("Keine Eingabegeräte gefunden.")));
        return vbox;
    }

    let list = ListBox::new();
    let scroll = ScrolledWindow::builder().vexpand(true).build();

    // if nothing configured yet, pre-select "pulse" (PipeWire follows GNOME default)
    let configured = config.borrow().microphone.clone();
    let effective = configured.clone().or_else(|| {
        if devices.iter().any(|d| d.name == "pulse") {
            Some("pulse".into())
        } else {
            devices.first().map(|d| d.name.clone())
        }
    });
    if configured.is_none() {
        if let Some(ref name) = effective {
            config.borrow_mut().microphone = Some(name.clone());
        }
    }

    // Tab-scoped registry of every per-row level meter cell. On window close
    // we walk this list and set each one to None so the cpal streams stop
    // even if the user forgot to toggle the test button off.
    type MeterCell = Rc<RefCell<Option<audio::LevelMeter>>>;
    let active_meters: Rc<RefCell<Vec<MeterCell>>> = Rc::new(RefCell::new(Vec::new()));

    // radio group: first CheckButton is the group leader; others join via set_group
    let mut group_leader: Option<CheckButton> = None;

    for dev_info in devices {
        let name = dev_info.name.clone();
        let row = ListBoxRow::new();
        let hbox = GtkBox::new(Orientation::Horizontal, 8);
        hbox.set_margin_top(4);
        hbox.set_margin_bottom(4);
        hbox.set_margin_start(8);
        hbox.set_margin_end(8);

        let check = CheckButton::new();
        if let Some(ref leader) = group_leader {
            check.set_group(Some(leader));
        } else {
            group_leader = Some(check.clone());
        }
        check.set_active(effective.as_deref() == Some(&name));
        {
            let cfg = Rc::clone(&config);
            let n = name.clone();
            check.connect_toggled(move |btn| {
                if btn.is_active() {
                    cfg.borrow_mut().microphone = Some(n.clone());
                }
            });
        }

        let name_lbl = Label::new(Some(&dev_info.display));
        name_lbl.set_hexpand(true);
        name_lbl.set_xalign(0.0);

        let level_bar = LevelBar::new();
        level_bar.set_min_value(0.0);
        level_bar.set_max_value(1.0);
        level_bar.set_size_request(120, -1);

        let test_btn = ToggleButton::with_label("Pegel testen");
        let meter_cell: MeterCell = Rc::new(RefCell::new(None));
        active_meters.borrow_mut().push(Rc::clone(&meter_cell));
        {
            let n = name.clone();
            let meter_cell = Rc::clone(&meter_cell);
            let bar = level_bar.clone();
            test_btn.connect_toggled(move |btn| {
                if btn.is_active() {
                    let Some(device) = audio::find_device_by_name(&n) else {
                        eprintln!("[settings] device gone: {n}");
                        btn.set_active(false);
                        return;
                    };
                    match audio::LevelMeter::start(&device) {
                        Ok(meter) => {
                            *meter_cell.borrow_mut() = Some(meter);
                            let mc = Rc::clone(&meter_cell);
                            let b = bar.clone();
                            glib::timeout_add_local(
                                std::time::Duration::from_millis(50),
                                move || match mc.borrow().as_ref() {
                                    Some(m) => {
                                        b.set_value((m.get() as f64 * 8.0).min(1.0));
                                        glib::ControlFlow::Continue
                                    }
                                    None => {
                                        b.set_value(0.0);
                                        glib::ControlFlow::Break
                                    }
                                },
                            );
                        }
                        Err(e) => {
                            eprintln!("[settings] level meter {n}: {e}");
                            btn.set_active(false);
                        }
                    }
                } else {
                    *meter_cell.borrow_mut() = None;
                    bar.set_value(0.0);
                }
            });
        }

        hbox.append(&check);
        hbox.append(&name_lbl);
        hbox.append(&test_btn);
        hbox.append(&level_bar);
        row.set_child(Some(&hbox));
        list.append(&row);
    }

    // Stop any still-running level meters when the settings window closes.
    {
        let am = Rc::clone(&active_meters);
        window.connect_close_request(move |_| {
            for cell in am.borrow().iter() {
                *cell.borrow_mut() = None;
            }
            glib::Propagation::Proceed
        });
    }

    scroll.set_child(Some(&list));
    vbox.append(&scroll);
    vbox
}

// ── Whisper-Tab ───────────────────────────────────────────────────────────

fn build_whisper_tab(config: Rc<RefCell<Config>>, whisper_port: u16) -> GtkBox {
    let vbox = GtkBox::new(Orientation::Vertical, 10);
    vbox.set_margin_top(12);
    vbox.set_margin_start(12);
    vbox.set_margin_end(12);

    let model_lbl = Label::new(Some("Modell:"));
    model_lbl.set_xalign(0.0);
    let model_combo = ComboBoxText::new();
    for m in whisper_models::MODELS {
        model_combo.append(Some(m.id), m.id);
    }
    model_combo.set_active_id(Some(&config.borrow().model));

    // Download status + buttons live below the dropdown and refresh whenever
    // the user changes the selection.
    let status_lbl = Label::new(None);
    status_lbl.set_xalign(0.0);
    status_lbl.set_wrap(true);

    let size_lbl = Label::new(None);
    size_lbl.set_xalign(0.0);

    let download_btn = Button::with_label("Modell herunterladen");
    let delete_btn = Button::with_label("Modell löschen");
    let spinner = Spinner::new();
    spinner.set_visible(false);

    let action_row = GtkBox::new(Orientation::Horizontal, 8);
    action_row.append(&download_btn);
    action_row.append(&delete_btn);
    action_row.append(&spinner);

    let refresh_state = {
        let status_lbl = status_lbl.clone();
        let size_lbl = size_lbl.clone();
        let download_btn = download_btn.clone();
        let delete_btn = delete_btn.clone();
        let model_combo = model_combo.clone();
        move || {
            let Some(id) = model_combo.active_id() else { return };
            let id = id.to_string();
            size_lbl.set_text(&format!("Größe: {}", whisper_models::size_label(&id)));
            if whisper_models::is_downloaded(&id) {
                status_lbl.set_markup("Status: <b>✓ heruntergeladen</b>");
                download_btn.set_sensitive(false);
                delete_btn.set_sensitive(true);
            } else {
                status_lbl.set_markup("Status: <b>✗ nicht vorhanden</b>");
                download_btn.set_sensitive(true);
                delete_btn.set_sensitive(false);
            }
        }
    };
    refresh_state();

    {
        let cfg = Rc::clone(&config);
        let refresh_state = refresh_state.clone();
        model_combo.connect_changed(move |cb| {
            if let Some(m) = cb.active_id() {
                cfg.borrow_mut().model = m.to_string();
                refresh_state();
            }
        });
    }

    {
        let spinner = spinner.clone();
        let download_btn = download_btn.clone();
        let delete_btn = delete_btn.clone();
        let status_lbl = status_lbl.clone();
        let model_combo = model_combo.clone();
        let refresh_state = refresh_state.clone();
        download_btn.connect_clicked(move |btn| {
            let Some(id) = model_combo.active_id() else { return };
            let id = id.to_string();
            btn.set_sensitive(false);
            delete_btn.set_sensitive(false);
            spinner.set_visible(true);
            spinner.start();
            status_lbl.set_text(&format!(
                "Lade {id} herunter — kann ein paar Minuten dauern…"
            ));

            let (tx, rx) = mpsc::channel::<Result<(), String>>();
            let id_clone = id.clone();
            std::thread::spawn(move || {
                let rt = match tokio::runtime::Runtime::new() {
                    Ok(r) => r,
                    Err(e) => {
                        let _ = tx.send(Err(format!("Tokio-Runtime: {e}")));
                        return;
                    }
                };
                let result = rt.block_on(async {
                    let client = WhisperClient::new(whisper_port);
                    client.ensure_model(&id_clone).await
                });
                let _ = tx.send(result);
            });

            let spinner = spinner.clone();
            let status_lbl = status_lbl.clone();
            let refresh_state = refresh_state.clone();
            glib::timeout_add_local(std::time::Duration::from_millis(250), move || {
                match rx.try_recv() {
                    Ok(Ok(())) => {
                        spinner.stop();
                        spinner.set_visible(false);
                        status_lbl.set_markup("<b>✓ Download abgeschlossen.</b>");
                        refresh_state();
                        glib::ControlFlow::Break
                    }
                    Ok(Err(e)) => {
                        spinner.stop();
                        spinner.set_visible(false);
                        status_lbl.set_text(&format!("✗ Fehler: {e}"));
                        refresh_state();
                        glib::ControlFlow::Break
                    }
                    Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        spinner.stop();
                        spinner.set_visible(false);
                        status_lbl.set_text("✗ Download-Thread abgebrochen.");
                        refresh_state();
                        glib::ControlFlow::Break
                    }
                }
            });
        });
    }

    {
        let model_combo = model_combo.clone();
        let status_lbl = status_lbl.clone();
        let refresh_state = refresh_state.clone();
        delete_btn.connect_clicked(move |_| {
            let Some(id) = model_combo.active_id() else { return };
            match whisper_models::delete_cache(&id) {
                Ok(()) => status_lbl.set_markup("<b>✓ Modell gelöscht.</b>"),
                Err(e) => status_lbl.set_text(&format!("✗ Löschen fehlgeschlagen: {e}")),
            }
            refresh_state();
        });
    }

    let lang_lbl = Label::new(Some("Sprache:"));
    lang_lbl.set_xalign(0.0);
    let lang_combo = ComboBoxText::new();
    for (id, label) in &[
        ("auto", "Automatisch erkennen"),
        ("de", "Deutsch"),
        ("en", "Englisch"),
        ("fr", "Französisch"),
        ("es", "Spanisch"),
        ("it", "Italienisch"),
        ("pt", "Portugiesisch"),
        ("nl", "Niederländisch"),
        ("pl", "Polnisch"),
        ("ru", "Russisch"),
        ("zh", "Chinesisch"),
        ("ja", "Japanisch"),
    ] {
        lang_combo.append(Some(id), label);
    }
    lang_combo.set_active_id(Some(&config.borrow().language));
    {
        let cfg = Rc::clone(&config);
        lang_combo.connect_changed(move |cb| {
            if let Some(l) = cb.active_id() {
                cfg.borrow_mut().language = l.to_string();
            }
        });
    }

    vbox.append(&model_lbl);
    vbox.append(&model_combo);
    vbox.append(&status_lbl);
    vbox.append(&size_lbl);
    vbox.append(&action_row);
    vbox.append(&Separator::new(Orientation::Horizontal));
    vbox.append(&build_gpu_section(Rc::clone(&config)));
    vbox.append(&Separator::new(Orientation::Horizontal));
    vbox.append(&lang_lbl);
    vbox.append(&lang_combo);
    vbox
}

// ── GPU-Sektion innerhalb des Whisper-Tabs ───────────────────────────────────

fn build_gpu_section(config: Rc<RefCell<Config>>) -> GtkBox {
    let section = GtkBox::new(Orientation::Vertical, 6);

    let heading = Label::new(None);
    heading.set_markup("<b>GPU-Beschleunigung</b>");
    heading.set_xalign(0.0);

    let status_lbl = Label::new(None);
    status_lbl.set_xalign(0.0);
    status_lbl.set_wrap(true);
    status_lbl.set_use_markup(true);

    let detail_lbl = Label::new(None);
    detail_lbl.set_xalign(0.0);
    detail_lbl.set_wrap(true);

    let install_btn = Button::with_label(&format!(
        "GPU-Unterstützung installieren ({})",
        gpu::estimated_download_label()
    ));
    let force_cpu_btn = CheckButton::with_label(
        "GPU deaktivieren und immer auf CPU rechnen (z. B. für Akkulaufzeit)",
    );
    force_cpu_btn.set_active(config.borrow().force_cpu);

    // Log-View ist initial versteckt; wird beim Klick auf Install eingeblendet.
    let log_buffer = TextBuffer::new(None);
    let log_view = TextView::with_buffer(&log_buffer);
    log_view.set_editable(false);
    log_view.set_monospace(true);
    let log_scroll = ScrolledWindow::builder()
        .height_request(160)
        .child(&log_view)
        .build();
    log_scroll.set_visible(false);

    let spinner = Spinner::new();
    spinner.set_visible(false);

    let action_row = GtkBox::new(Orientation::Horizontal, 8);
    action_row.append(&install_btn);
    action_row.append(&spinner);

    // Update der UI basierend auf Hardware-Detection + venv-Zustand.
    let refresh = {
        let status_lbl = status_lbl.clone();
        let detail_lbl = detail_lbl.clone();
        let install_btn = install_btn.clone();
        let force_cpu_btn = force_cpu_btn.clone();
        move || {
            let hw = gpu::detect_hardware();
            let active = gpu::libs_active_in_venv();
            let wheels = gpu::wheels_installed();

            match (&hw, active, wheels) {
                (gpu::NvidiaHardware::None, _, _) => {
                    status_lbl.set_markup("Verarbeitung: <b>CPU</b>");
                    detail_lbl.set_text(
                        "Keine NVIDIA-GPU im System gefunden — vooox läuft auf der CPU.",
                    );
                    install_btn.set_visible(false);
                    force_cpu_btn.set_visible(false);
                }
                (gpu::NvidiaHardware::NoDriver, _, _) => {
                    status_lbl.set_markup("Verarbeitung: <b>CPU</b> (Treiber fehlt)");
                    detail_lbl.set_text(
                        "NVIDIA-Karte erkannt, aber der proprietäre Treiber ist nicht installiert. \
                         Installiere ihn über deinen Paketmanager (z. B. `nvidia-driver-535`) \
                         und starte den Rechner neu.",
                    );
                    install_btn.set_visible(false);
                    force_cpu_btn.set_visible(false);
                }
                (gpu::NvidiaHardware::DriverTooOld { driver }, _, _) => {
                    status_lbl
                        .set_markup(&format!("Verarbeitung: <b>CPU</b> (Treiber {driver} zu alt)"));
                    detail_lbl.set_text(
                        "Der NVIDIA-Treiber muss mindestens Version 525 sein, um CUDA 12 zu \
                         unterstützen. Aktualisiere ihn über deinen Paketmanager.",
                    );
                    install_btn.set_visible(false);
                    force_cpu_btn.set_visible(false);
                }
                (gpu::NvidiaHardware::Ok { driver }, true, _) => {
                    status_lbl.set_markup(&format!(
                        "Verarbeitung: <b>GPU (CUDA)</b> — Treiber {driver}",
                    ));
                    detail_lbl.set_text(
                        "GPU-Beschleunigung ist aktiv. Bei größeren Modellen (medium / large-v3) \
                         macht das einen spürbaren Unterschied.",
                    );
                    install_btn.set_visible(false);
                    force_cpu_btn.set_visible(true);
                }
                (gpu::NvidiaHardware::Ok { driver }, false, true) => {
                    // Wheels da, aber ctranslate2 sieht nichts — meist
                    // Treiber-Library-Mismatch oder force-cpu noch aus dem
                    // vorigen Sidecar.
                    status_lbl.set_markup(&format!(
                        "Verarbeitung: <b>CPU</b> (Treiber {driver}, CUDA-Libraries installiert)",
                    ));
                    detail_lbl.set_text(
                        "Die CUDA-Pakete sind installiert, vooox nutzt aber gerade die CPU. \
                         Falls die GPU-Deaktivierung aus ist: vooox neu starten — der Sidecar \
                         läuft noch mit der alten Konfiguration.",
                    );
                    install_btn.set_visible(false);
                    force_cpu_btn.set_visible(true);
                }
                (gpu::NvidiaHardware::Ok { driver }, false, false) => {
                    status_lbl.set_markup(&format!(
                        "Verarbeitung: <b>CPU</b> — GPU verfügbar (Treiber {driver})",
                    ));
                    detail_lbl.set_text(
                        "Du kannst die CUDA-Libraries jetzt nachinstallieren, um Transkriptionen \
                         deutlich zu beschleunigen. Nach der Installation vooox einmal neu \
                         starten.",
                    );
                    install_btn.set_visible(true);
                    install_btn.set_sensitive(true);
                    force_cpu_btn.set_visible(false);
                }
            }
        }
    };
    refresh();

    // Force-CPU-Toggle: speichert Config; greift beim nächsten Sidecar-Start.
    {
        let cfg = Rc::clone(&config);
        let refresh = refresh.clone();
        force_cpu_btn.connect_toggled(move |btn| {
            cfg.borrow_mut().force_cpu = btn.is_active();
            refresh();
        });
    }

    // Install-Button → CUDA-Wheels installieren mit Live-Log.
    {
        let spinner = spinner.clone();
        let log_buffer = log_buffer.clone();
        let log_view = log_view.clone();
        let log_scroll = log_scroll.clone();
        let detail_lbl = detail_lbl.clone();
        let refresh = refresh.clone();
        install_btn.connect_clicked(move |btn| {
            btn.set_sensitive(false);
            spinner.set_visible(true);
            spinner.start();
            log_scroll.set_visible(true);
            log_buffer.set_text("");
            detail_lbl.set_text(
                "Lade CUDA- und cuDNN-Libraries herunter — kann ein paar Minuten dauern.",
            );

            let (tx, rx) = mpsc::channel::<gpu::InstallMsg>();
            gpu::spawn_cuda_install_thread(tx);

            let spinner = spinner.clone();
            let log_buffer = log_buffer.clone();
            let log_view = log_view.clone();
            let detail_lbl = detail_lbl.clone();
            let refresh = refresh.clone();
            let btn = btn.clone();
            glib::timeout_add_local(std::time::Duration::from_millis(120), move || {
                let mut done = false;
                let mut had_error = false;
                while let Ok(m) = rx.try_recv() {
                    match m {
                        gpu::InstallMsg::Line(line) => append_gpu_log(&log_buffer, &log_view, &line),
                        gpu::InstallMsg::Done => {
                            done = true;
                            break;
                        }
                        gpu::InstallMsg::Error(e) => {
                            append_gpu_log(
                                &log_buffer,
                                &log_view,
                                &format!("\n✗ FEHLER: {e}"),
                            );
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
                        btn.set_label("Erneut versuchen");
                        btn.set_sensitive(true);
                        detail_lbl.set_text(
                            "Installation fehlgeschlagen — Details siehe Log oben.",
                        );
                    } else {
                        append_gpu_log(&log_buffer, &log_view, "");
                        append_gpu_log(
                            &log_buffer,
                            &log_view,
                            "✓ CUDA-Libraries installiert. vooox neu starten, damit der \
                             Sidecar die GPU benutzt.",
                        );
                        detail_lbl.set_text(
                            "Fertig — vooox einmal neu starten, dann läuft die Transkription \
                             auf der GPU.",
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

    section.append(&heading);
    section.append(&status_lbl);
    section.append(&detail_lbl);
    section.append(&action_row);
    section.append(&log_scroll);
    section.append(&force_cpu_btn);
    section
}

fn append_gpu_log(buf: &TextBuffer, view: &TextView, line: &str) {
    let mut iter = buf.end_iter();
    if buf.char_count() > 0 {
        buf.insert(&mut iter, "\n");
    }
    let line_start = buf.create_mark(None, &iter, true);
    buf.insert(&mut iter, line);
    view.scroll_to_mark(&line_start, 0.0, true, 0.0, 1.0);
    buf.delete_mark(&line_start);
}

// ── Shortcut-Tab ──────────────────────────────────────────────────────────

fn build_shortcut_tab(config: Rc<RefCell<Config>>) -> GtkBox {
    let vbox = GtkBox::new(Orientation::Vertical, 10);
    vbox.set_margin_top(12);
    vbox.set_margin_start(12);
    vbox.set_margin_end(12);

    let info = Label::new(Some(
        "Format: ctrl+shift+space\nMögliche Modifier: ctrl, shift, alt, super",
    ));
    info.set_xalign(0.0);
    info.set_wrap(true);

    let entry = Entry::new();
    entry.set_text(&config.borrow().shortcut);
    {
        let cfg = Rc::clone(&config);
        entry.connect_changed(move |e| {
            cfg.borrow_mut().shortcut = e.text().to_string();
        });
    }

    vbox.append(&info);
    vbox.append(&entry);
    vbox
}

// ── Allgemein-Tab ─────────────────────────────────────────────────────────

fn build_general_tab(config: Rc<RefCell<Config>>) -> GtkBox {
    let vbox = GtkBox::new(Orientation::Vertical, 10);
    vbox.set_margin_top(12);
    vbox.set_margin_start(12);
    vbox.set_margin_end(12);

    let autostart_btn = CheckButton::with_label("Automatisch beim Login starten");
    autostart_btn.set_active(config.borrow().autostart);
    {
        let cfg = Rc::clone(&config);
        autostart_btn.connect_toggled(move |btn| {
            cfg.borrow_mut().autostart = btn.is_active();
        });
    }
    vbox.append(&autostart_btn);

    vbox.append(&Separator::new(Orientation::Horizontal));

    let ptt_btn = CheckButton::with_label("Push-to-Talk aktivieren");
    ptt_btn.set_active(config.borrow().push_to_talk_enabled);

    let ptt_desc = Label::new(Some(
        "Beim langen Halten des Shortcuts (länger als die unten eingestellte Schwelle) \
         wechselt vooox in den Push-to-Talk-Modus: Sobald du den Shortcut loslässt, \
         endet die Aufnahme sofort. Ein kurzer Druck schaltet wie gewohnt um \
         (Aufnahme starten/stoppen). Während Push-to-Talk aktiv ist, leuchtet die \
         Statusanzeige lila.",
    ));
    ptt_desc.set_xalign(0.0);
    ptt_desc.set_wrap(true);

    let threshold_lbl = Label::new(Some("Schwelle (ms):"));
    threshold_lbl.set_xalign(0.0);

    let adj = Adjustment::new(
        config.borrow().push_to_talk_threshold_ms as f64,
        100.0,  // min
        3000.0, // max
        50.0,   // step
        100.0,  // page
        0.0,
    );
    let threshold_spin = SpinButton::new(Some(&adj), 1.0, 0);

    let threshold_row = GtkBox::new(Orientation::Horizontal, 8);
    threshold_row.append(&threshold_lbl);
    threshold_row.append(&threshold_spin);

    {
        let cfg = Rc::clone(&config);
        let desc = ptt_desc.clone();
        let row = threshold_row.clone();
        ptt_btn.connect_toggled(move |btn| {
            let enabled = btn.is_active();
            cfg.borrow_mut().push_to_talk_enabled = enabled;
            desc.set_sensitive(enabled);
            row.set_sensitive(enabled);
        });
    }
    {
        let cfg = Rc::clone(&config);
        threshold_spin.connect_value_changed(move |sb| {
            cfg.borrow_mut().push_to_talk_threshold_ms = sb.value() as u32;
        });
    }

    let initial = config.borrow().push_to_talk_enabled;
    ptt_desc.set_sensitive(initial);
    threshold_row.set_sensitive(initial);

    vbox.append(&ptt_btn);
    vbox.append(&ptt_desc);
    vbox.append(&threshold_row);

    vbox.append(&Separator::new(Orientation::Horizontal));

    let paste_lbl = Label::new(Some("Automatisches Einfügen"));
    paste_lbl.set_xalign(0.0);
    paste_lbl.add_css_class("heading");

    let paste_desc = Label::new(Some(
        "Nach der Transkription wird der Text per simuliertem Strg+V direkt \
         im zuvor fokussierten Fenster eingefügt (benötigt xdotool, X11). \
         Du kannst es pro Aufnahme-Modus separat aktivieren.",
    ));
    paste_desc.set_xalign(0.0);
    paste_desc.set_wrap(true);

    let paste_toggle_btn =
        CheckButton::with_label("Im Toggle-Modus (kurzer Druck) automatisch einfügen");
    paste_toggle_btn.set_active(config.borrow().auto_paste_toggle);
    {
        let cfg = Rc::clone(&config);
        paste_toggle_btn.connect_toggled(move |btn| {
            cfg.borrow_mut().auto_paste_toggle = btn.is_active();
        });
    }

    let paste_ptt_btn =
        CheckButton::with_label("Im Push-to-Talk-Modus automatisch einfügen");
    paste_ptt_btn.set_active(config.borrow().auto_paste_ptt);
    {
        let cfg = Rc::clone(&config);
        paste_ptt_btn.connect_toggled(move |btn| {
            cfg.borrow_mut().auto_paste_ptt = btn.is_active();
        });
    }

    vbox.append(&paste_lbl);
    vbox.append(&paste_desc);
    vbox.append(&paste_toggle_btn);
    vbox.append(&paste_ptt_btn);

    vbox
}
