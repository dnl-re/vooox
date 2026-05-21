use crate::audio;
use crate::config::Config;
use crate::whisper_client::WhisperClient;
use crate::whisper_models;
use crate::x11_window;
use glib;
use gtk4::prelude::*;
use gtk4::{
    Adjustment, Application, ApplicationWindow, Box as GtkBox, Button, CheckButton, ComboBoxText,
    Entry, Label, LevelBar, ListBox, ListBoxRow, Notebook, Orientation, ScrolledWindow, Separator,
    SpinButton, Spinner, ToggleButton,
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
            .default_width(760)
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
    vbox.append(&lang_lbl);
    vbox.append(&lang_combo);
    vbox
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
