use crate::audio;
use crate::config::Config;
use glib;
use gtk4::prelude::*;
use gtk4::{
    Adjustment, Application, ApplicationWindow, Box as GtkBox, Button, CheckButton, ComboBoxText,
    Entry, Label, LevelBar, ListBox, ListBoxRow, Notebook, Orientation, ScrolledWindow, Separator,
    SpinButton,
};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

pub struct SettingsWindow {
    window: ApplicationWindow,
}

impl SettingsWindow {
    pub fn new(app: &Application, config: Rc<RefCell<Config>>, _whisper_port: u16) -> Self {
        let window = ApplicationWindow::builder()
            .application(app)
            .title("vooox — Einstellungen")
            .default_width(600)
            .default_height(480)
            .build();

        let notebook = Notebook::new();
        notebook.append_page(
            &build_general_tab(Rc::clone(&config)),
            Some(&Label::new(Some("Allgemein"))),
        );
        notebook.append_page(
            &build_microphone_tab(Rc::clone(&config)),
            Some(&Label::new(Some("Mikrofon"))),
        );
        notebook.append_page(
            &build_whisper_tab(Rc::clone(&config)),
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
    }
}

// ── Mikrofon-Tab ──────────────────────────────────────────────────────────

fn build_microphone_tab(config: Rc<RefCell<Config>>) -> GtkBox {
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

    // shared level store: device_name → level Arc
    let level_store: Arc<Mutex<Vec<(String, Arc<Mutex<f32>>)>>> =
        Arc::new(Mutex::new(Vec::new()));

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
        // join the radio group so only one can be active at a time
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

        if let Some(device) = audio::find_device_by_name(&name) {
            if let Ok(meter) = audio::LevelMeter::start(&device) {
                let lv = Arc::clone(&meter.level);
                level_store.lock().unwrap().push((name.clone(), lv));
                std::mem::forget(meter); // keep alive for settings lifetime
            }
        }
        {
            let store = Arc::clone(&level_store);
            let bar = level_bar.clone();
            let n = name.clone();
            glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
                if let Some((_, lv)) = store.lock().unwrap().iter().find(|(nm, _)| nm == &n) {
                    bar.set_value((*lv.lock().unwrap() as f64 * 8.0).min(1.0));
                }
                glib::ControlFlow::Continue
            });
        }

        hbox.append(&check);
        hbox.append(&name_lbl);
        hbox.append(&level_bar);
        row.set_child(Some(&hbox));
        list.append(&row);
    }

    scroll.set_child(Some(&list));
    vbox.append(&scroll);
    vbox
}

// ── Whisper-Tab ───────────────────────────────────────────────────────────

fn build_whisper_tab(config: Rc<RefCell<Config>>) -> GtkBox {
    let vbox = GtkBox::new(Orientation::Vertical, 10);
    vbox.set_margin_top(12);
    vbox.set_margin_start(12);
    vbox.set_margin_end(12);

    let model_lbl = Label::new(Some("Modell:"));
    model_lbl.set_xalign(0.0);
    let model_combo = ComboBoxText::new();
    for m in &["tiny", "base", "small", "medium", "large-v2", "large-v3"] {
        model_combo.append(Some(m), m);
    }
    model_combo.set_active_id(Some(&config.borrow().model));
    {
        let cfg = Rc::clone(&config);
        model_combo.connect_changed(move |cb| {
            if let Some(m) = cb.active_id() {
                cfg.borrow_mut().model = m.to_string();
            }
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
