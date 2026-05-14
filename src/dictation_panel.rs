use crate::audio;
use crate::config::Config;
use crate::history::{History, HistoryEntry};
use glib;
use gtk4::prelude::*;
use gtk4::{
    Application, ApplicationWindow, Box as GtkBox, Button, CssProvider, Label, LevelBar, ListBox,
    ListBoxRow, Orientation, ScrolledWindow, Separator, TextView,
};
use std::cell::RefCell;
use std::rc::Rc;

const CSS: &str = r#"
.status-rec  { color: #ff4444; font-weight: bold; }
.status-proc { color: #ffaa00; font-weight: bold; }
.status-idle { color: #888888; }
.copy-btn-done { background-color: #26a269; color: white; }
.history-time  { font-size: 11px; color: #888888; }
"#;

pub struct DictationPanel {
    app: Application,
    window: ApplicationWindow,
    status_label: Label,
    timer_label: Label,
    level_bar: LevelBar,
    text_view: TextView,
    copy_btn: Button,
    history_list: ListBox,
    level_meter: Rc<RefCell<Option<audio::LevelMeter>>>,
    timer_source: Rc<RefCell<Option<glib::SourceId>>>,
    timer_seconds: Rc<RefCell<u32>>,
}

impl DictationPanel {
    pub fn new(app: &Application) -> Self {
        let provider = CssProvider::new();
        provider.load_from_data(CSS);
        if let Some(display) = gtk4::gdk::Display::default() {
            gtk4::style_context_add_provider_for_display(
                &display,
                &provider,
                gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }

        // ── header ────────────────────────────────────────────────────────
        let status_label = Label::new(Some("○ Bereit"));
        status_label.add_css_class("status-idle");

        let timer_label = Label::new(Some(""));
        timer_label.set_hexpand(true);
        timer_label.set_xalign(1.0);

        let level_bar = LevelBar::new();
        level_bar.set_min_value(0.0);
        level_bar.set_max_value(1.0);
        level_bar.set_size_request(100, -1);
        level_bar.set_valign(gtk4::Align::Center);

        let header_box = GtkBox::new(Orientation::Horizontal, 8);
        header_box.set_margin_top(8);
        header_box.set_margin_bottom(8);
        header_box.set_margin_start(12);
        header_box.set_margin_end(12);
        header_box.append(&status_label);
        header_box.append(&timer_label);
        header_box.append(&level_bar);

        // ── transcript text view ──────────────────────────────────────────
        let text_view = TextView::new();
        text_view.set_editable(true);
        text_view.set_wrap_mode(gtk4::WrapMode::WordChar);
        text_view.set_left_margin(12);
        text_view.set_right_margin(12);
        text_view.set_top_margin(8);
        text_view.set_bottom_margin(8);

        let text_scroll = ScrolledWindow::builder()
            .vexpand(true)
            .min_content_height(80)
            .build();
        text_scroll.set_child(Some(&text_view));

        // ── buttons ───────────────────────────────────────────────────────
        let clear_btn = Button::with_label("Leeren");
        let copy_btn = Button::with_label("Kopieren");
        copy_btn.set_halign(gtk4::Align::End);

        let spacer = GtkBox::new(Orientation::Horizontal, 0);
        spacer.set_hexpand(true);

        let btn_box = GtkBox::new(Orientation::Horizontal, 8);
        btn_box.set_margin_top(4);
        btn_box.set_margin_bottom(4);
        btn_box.set_margin_start(12);
        btn_box.set_margin_end(12);
        btn_box.append(&clear_btn);
        btn_box.append(&spacer);
        btn_box.append(&copy_btn);

        // ── history ───────────────────────────────────────────────────────
        let history_hdr = Label::new(Some("Verlauf"));
        history_hdr.set_xalign(0.0);
        history_hdr.set_margin_start(12);
        history_hdr.set_margin_top(8);
        history_hdr.set_margin_bottom(4);

        let history_list = ListBox::new();
        history_list.set_selection_mode(gtk4::SelectionMode::None);

        let history_scroll = ScrolledWindow::builder()
            .vexpand(true)
            .min_content_height(120)
            .build();
        history_scroll.set_child(Some(&history_list));

        // ── assemble ──────────────────────────────────────────────────────
        let vbox = GtkBox::new(Orientation::Vertical, 0);
        vbox.append(&header_box);
        vbox.append(&Separator::new(Orientation::Horizontal));
        vbox.append(&text_scroll);
        vbox.append(&btn_box);
        vbox.append(&Separator::new(Orientation::Horizontal));
        vbox.append(&history_hdr);
        vbox.append(&history_scroll);

        let window = ApplicationWindow::builder()
            .application(app)
            .title("vooox")
            .default_width(480)
            .default_height(520)
            .build();
        window.set_child(Some(&vbox));

        // ── button handlers ───────────────────────────────────────────────
        {
            let tv = text_view.clone();
            clear_btn.connect_clicked(move |_| {
                tv.buffer().set_text("");
            });
        }
        {
            let tv = text_view.clone();
            copy_btn.connect_clicked(move |btn| {
                let buf = tv.buffer();
                let text = buf.text(&buf.start_iter(), &buf.end_iter(), false);
                if let Some(display) = gtk4::gdk::Display::default() {
                    display.clipboard().set_text(text.as_str());
                }
                btn.set_label("✓ Kopiert!");
                btn.add_css_class("copy-btn-done");
                let b = btn.clone();
                glib::timeout_add_local_once(std::time::Duration::from_millis(1500), move || {
                    b.set_label("Kopieren");
                    b.remove_css_class("copy-btn-done");
                });
            });
        }

        DictationPanel {
            app: app.clone(),
            window,
            status_label,
            timer_label,
            level_bar,
            text_view,
            copy_btn,
            history_list,
            level_meter: Rc::new(RefCell::new(None)),
            timer_source: Rc::new(RefCell::new(None)),
            timer_seconds: Rc::new(RefCell::new(0)),
        }
    }

    pub fn show_recording(&self, device: &cpal::Device) {
        // stop any leftover timer from previous session
        if let Some(id) = self.timer_source.borrow_mut().take() {
            id.remove();
        }

        // clear text for new dictation
        self.text_view.buffer().set_text("");

        // status
        self.status_label.set_text("● Aufnahme");
        self.status_label.remove_css_class("status-proc");
        self.status_label.remove_css_class("status-idle");
        self.status_label.add_css_class("status-rec");

        // start level meter
        *self.level_meter.borrow_mut() = None; // drop previous stream
        match audio::LevelMeter::start(device) {
            Ok(meter) => {
                *self.level_meter.borrow_mut() = Some(meter);
                let meter_rc = Rc::clone(&self.level_meter);
                let bar = self.level_bar.clone();
                glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
                    if let Some(ref m) = *meter_rc.borrow() {
                        bar.set_value(m.get() as f64);
                        glib::ControlFlow::Continue
                    } else {
                        bar.set_value(0.0);
                        glib::ControlFlow::Break
                    }
                });
            }
            Err(e) => eprintln!("[panel] level meter: {e}"),
        }

        // start 1-second timer
        *self.timer_seconds.borrow_mut() = 0;
        self.timer_label.set_text("00:00");
        let secs_rc = Rc::clone(&self.timer_seconds);
        let lbl = self.timer_label.clone();
        let id = glib::timeout_add_local(std::time::Duration::from_secs(1), move || {
            let mut s = secs_rc.borrow_mut();
            *s += 1;
            lbl.set_text(&format!("{:02}:{:02}", *s / 60, *s % 60));
            glib::ControlFlow::Continue
        });
        *self.timer_source.borrow_mut() = Some(id);

        self.window.present();
    }

    pub fn show_processing(&self) {
        // stop level meter (level-bar update timer will see None and break)
        *self.level_meter.borrow_mut() = None;

        // stop timer
        if let Some(id) = self.timer_source.borrow_mut().take() {
            id.remove();
        }

        self.status_label.set_text("⏳ Verarbeitung…");
        self.status_label.remove_css_class("status-rec");
        self.status_label.remove_css_class("status-idle");
        self.status_label.add_css_class("status-proc");
    }

    /// Append a whisper segment to the live transcript.
    pub fn append_segment(&self, seg: &str) {
        let buf = self.text_view.buffer();
        let existing = buf.text(&buf.start_iter(), &buf.end_iter(), false);
        let to_insert = if existing.is_empty()
            || existing.ends_with(' ')
            || seg.starts_with(' ')
        {
            seg.to_string()
        } else {
            format!(" {seg}")
        };
        buf.insert(&mut buf.end_iter(), &to_insert);
    }

    /// Called when transcription is complete: reset status, auto-copy, add history row.
    pub fn finish(&self, full_text: &str, cfg: &Config, history: &mut History) {
        self.status_label.set_text("○ Bereit");
        self.status_label.remove_css_class("status-proc");
        self.status_label.remove_css_class("status-rec");
        self.status_label.add_css_class("status-idle");
        self.timer_label.set_text("");

        if full_text.is_empty() {
            return;
        }

        // auto-copy to clipboard
        if let Some(display) = gtk4::gdk::Display::default() {
            display.clipboard().set_text(full_text);
        }

        // desktop notification
        let preview: String = full_text.chars().take(60).collect();
        let body = if full_text.len() > 60 {
            format!("{}… — in Zwischenablage kopiert", preview)
        } else {
            format!("{} — in Zwischenablage kopiert", preview)
        };
        let notif = gtk4::gio::Notification::new("vooox");
        notif.set_body(Some(&body));
        self.app.send_notification(None, &notif);

        // flash copy button
        self.copy_btn.set_label("✓ Kopiert!");
        self.copy_btn.add_css_class("copy-btn-done");
        let btn = self.copy_btn.clone();
        glib::timeout_add_local_once(std::time::Duration::from_millis(1500), move || {
            btn.set_label("Kopieren");
            btn.remove_css_class("copy-btn-done");
        });

        // persist and show in history
        let entry = HistoryEntry {
            text: full_text.to_string(),
            timestamp: crate::history::now_rfc3339(),
            model: cfg.model.clone(),
            language: cfg.language.clone(),
        };
        history.push(entry.clone());
        self.history_list.prepend(&make_history_row(&entry));
    }

    /// Populate history list from persisted entries (newest first).
    pub fn load_history(&self, history: &History) {
        let entries: Vec<_> = history.entries().collect();
        for entry in entries.iter().rev() {
            self.history_list.append(&make_history_row(entry));
        }
    }

    pub fn present(&self) {
        self.window.present();
    }
}

fn make_history_row(entry: &HistoryEntry) -> ListBoxRow {
    let row = ListBoxRow::new();
    let hbox = GtkBox::new(Orientation::Horizontal, 8);
    hbox.set_margin_top(4);
    hbox.set_margin_bottom(4);
    hbox.set_margin_start(8);
    hbox.set_margin_end(8);

    // time label (HH:MM from timestamp)
    let ts_display = entry
        .timestamp
        .get(11..16)
        .unwrap_or(&entry.timestamp)
        .to_string();
    let time_lbl = Label::new(Some(&ts_display));
    time_lbl.add_css_class("history-time");
    time_lbl.set_valign(gtk4::Align::Start);

    let text_lbl = Label::new(Some(&entry.text));
    text_lbl.set_hexpand(true);
    text_lbl.set_xalign(0.0);
    text_lbl.set_wrap(true);
    text_lbl.set_max_width_chars(50);

    let copy_btn = Button::with_label("📋");
    copy_btn.set_valign(gtk4::Align::Start);
    {
        let text = entry.text.clone();
        copy_btn.connect_clicked(move |btn| {
            if let Some(display) = gtk4::gdk::Display::default() {
                display.clipboard().set_text(&text);
                btn.set_label("✓");
                let b = btn.clone();
                glib::timeout_add_local_once(std::time::Duration::from_secs(1), move || {
                    b.set_label("📋");
                });
            }
        });
    }

    hbox.append(&time_lbl);
    hbox.append(&text_lbl);
    hbox.append(&copy_btn);
    row.set_child(Some(&hbox));
    row
}
