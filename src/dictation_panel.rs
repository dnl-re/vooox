use crate::audio;
use crate::config::Config;
use crate::history::{History, HistoryEntry};
use crate::window_state::{monitor_key, WindowState};
use std::rc::Rc;
use std::cell::RefCell;
use glib;
use gtk4::prelude::*;
use gtk4::{
    Application, ApplicationWindow, Box as GtkBox, Button, CssProvider, Expander, Label, LevelBar,
    ListBox, ListBoxRow, Orientation, ScrolledWindow, Separator, TextView,
};

const CSS: &str = r#"
.status-rec  { color: #ff4444; font-weight: bold; }
.status-proc { color: #ffaa00; font-weight: bold; }
.status-idle { color: #888888; }
.copy-btn-done { background-color: #26a269; color: white; }
.history-time  { font-size: 11px; color: #888888; }
.toast { color: #26a269; font-size: 14px; }
window { background: transparent; }
.panel-root {
    border-radius: 12px;
    border: 1px solid alpha(currentColor, 0.12);
}
"#;

pub struct DictationPanel {
    window: ApplicationWindow,
    status_label: Label,
    timer_label: Label,
    level_bar: LevelBar,
    text_view: TextView,
    toast_label: Label,
    history_list: ListBox,
    level_meter: Rc<RefCell<Option<audio::LevelMeter>>>,
    timer_source: Rc<RefCell<Option<glib::SourceId>>>,
    timer_seconds: Rc<RefCell<u32>>,
    // Text that existed before current recording; empty in replace-mode.
    // set_transcript() writes base_text + new_text so streaming never duplicates.
    base_text: Rc<RefCell<String>>,
    win_state: Rc<RefCell<WindowState>>,
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

        // ── toast ─────────────────────────────────────────────────────────
        let toast_label = Label::new(None);
        toast_label.add_css_class("toast");
        toast_label.set_hexpand(true);
        toast_label.set_xalign(0.5);
        toast_label.set_margin_top(4);
        toast_label.set_margin_bottom(4);

        // ── history ───────────────────────────────────────────────────────
        let history_list = ListBox::new();
        history_list.set_selection_mode(gtk4::SelectionMode::None);

        let history_scroll = ScrolledWindow::builder()
            .vexpand(false)
            .min_content_height(120)
            .max_content_height(120)
            .build();
        history_scroll.set_child(Some(&history_list));

        let history_expander = Expander::new(Some("Verlauf"));
        history_expander.set_expanded(false);
        history_expander.set_margin_start(8);
        history_expander.set_margin_end(8);
        history_expander.set_margin_top(4);
        history_expander.set_margin_bottom(4);
        history_expander.set_child(Some(&history_scroll));

        // ── assemble ──────────────────────────────────────────────────────
        let vbox = GtkBox::new(Orientation::Vertical, 0);
        vbox.add_css_class("background"); // GTK4 built-in: applies @window_bg_color
        vbox.add_css_class("panel-root");
        vbox.append(&header_box);
        vbox.append(&Separator::new(Orientation::Horizontal));
        vbox.append(&text_scroll);
        vbox.append(&toast_label);
        vbox.append(&Separator::new(Orientation::Horizontal));
        vbox.append(&history_expander);

        let window = ApplicationWindow::builder()
            .application(app)
            .title("vooox")
            .default_width(480)
            .default_height(260)
            .decorated(false)
            .build();
        window.set_child(Some(&vbox));

        let win_state = Rc::new(RefCell::new(WindowState::load()));

        // hide instead of destroy when the user closes the window — and
        // persist the current position keyed by monitor.
        {
            let state = Rc::clone(&win_state);
            window.connect_close_request(move |win| {
                save_window_position(win, &state);
                win.hide();
                glib::Propagation::Stop
            });
        }

        // drag header to move the undecorated window
        {
            let win = window.clone();
            let drag = gtk4::GestureClick::new();
            drag.set_button(1);
            drag.connect_pressed(move |gesture, _n, x, y| {
                if let Some(event) = gesture.last_event(None) {
                    if let Some(surface) = win.surface() {
                        use gtk4::gdk::prelude::ToplevelExt;
                        if let Ok(toplevel) = surface.downcast::<gtk4::gdk::Toplevel>() {
                            if let Some(device) = event.device() {
                                toplevel.begin_move(&device, 1, x, y, event.time());
                            }
                        }
                    }
                }
            });
            header_box.add_controller(drag);
        }



        DictationPanel {
            window,
            status_label,
            timer_label,
            level_bar,
            text_view,
            toast_label,
            history_list,
            level_meter: Rc::new(RefCell::new(None)),
            timer_source: Rc::new(RefCell::new(None)),
            timer_seconds: Rc::new(RefCell::new(0)),
            base_text: Rc::new(RefCell::new(String::new())),
            win_state,
        }
    }

    pub fn show_recording(&self, device: &cpal::Device) {
        // stop any leftover timer from previous session
        if let Some(id) = self.timer_source.borrow_mut().take() {
            id.remove();
        }

        // Append mode: cursor is in the text view — keep existing text as base.
        // Replace mode: clear buffer.
        if self.text_view.has_focus() {
            let buf = self.text_view.buffer();
            let mut existing: String = buf.text(&buf.start_iter(), &buf.end_iter(), false).into();
            if !existing.is_empty() && !existing.ends_with(' ') {
                existing.push(' ');
                buf.insert(&mut buf.end_iter(), " ");
            }
            *self.base_text.borrow_mut() = existing;
        } else {
            self.text_view.buffer().set_text("");
            *self.base_text.borrow_mut() = String::new();
        }

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

        // Capture the X11 window the user is currently working in so we can
        // restore its focus after we present ourselves on top.
        let prev_active: Option<String> = std::process::Command::new("xdotool")
            .arg("getactivewindow")
            .output()
            .ok()
            .and_then(|o| {
                let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                if s.is_empty() { None } else { Some(s) }
            });

        // Only (re)position when the window is being brought back from a hidden
        // state — if it's already visible, the user's drag-placement stays put.
        let was_hidden = !self.window.is_visible();

        // Clear focus child so GTK doesn't call XSetInputFocus() on the TextView.
        gtk4::prelude::GtkWindowExt::set_focus(&self.window, None::<&gtk4::Widget>);

        // present() maps + raises the window reliably (works on first and Nth call).
        // It also briefly takes focus — we restore it below.
        self.window.present();

        let win = self.window.clone();
        let state = Rc::clone(&self.win_state);
        glib::timeout_add_local_once(std::time::Duration::from_millis(50), move || {
            if was_hidden {
                position_for_cursor(&win, &state);
            }

            let our_xid = window_xid(&win);

            // Force the window onto the active stack via EWMH. windowactivate
            // sends _NET_ACTIVE_WINDOW with the current X server time, which
            // Mutter accepts even when GTK's own present() gets demoted to a
            // "window ready" notification.
            if let Some(xid) = our_xid {
                let _ = std::process::Command::new("xdotool")
                    .args(["windowactivate", "--sync", &xid.to_string()])
                    .status();
            }

            // Hand keyboard focus back to the previous app without lowering us.
            if let Some(ref xid) = prev_active {
                let _ = std::process::Command::new("xdotool")
                    .args(["windowfocus", "--sync", xid])
                    .status();
            }

            // Re-raise ourselves so the visual stack puts us on top.
            if let Some(xid) = our_xid {
                let _ = std::process::Command::new("xdotool")
                    .args(["windowraise", &xid.to_string()])
                    .status();
            }
        });
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

    pub fn text_view_text(&self) -> String {
        let buf = self.text_view.buffer();
        buf.text(&buf.start_iter(), &buf.end_iter(), false).into()
    }

    /// Replace the streaming portion of the transcript.
    /// Always writes base_text + text so repeated calls never duplicate content.
    pub fn set_transcript(&self, text: &str) {
        let base = self.base_text.borrow();
        let full = format!("{}{}", *base, text);
        self.text_view.buffer().set_text(&full);
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
    pub fn finish(&self, full_text: &str, cfg: &Config, history: Rc<RefCell<History>>) {
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

        // in-window toast
        self.toast_label.set_text("✓ In Zwischenablage kopiert");
        let lbl = self.toast_label.clone();
        glib::timeout_add_local_once(std::time::Duration::from_secs(3), move || {
            lbl.set_text("");
        });

        // persist and show in history
        let entry = HistoryEntry {
            text: full_text.to_string(),
            timestamp: crate::history::now_rfc3339(),
            model: cfg.model.clone(),
            language: cfg.language.clone(),
        };
        history.borrow_mut().push(entry.clone());
        self.history_list
            .prepend(&make_history_row(&entry, &self.history_list, Rc::clone(&history)));
    }

    /// Populate history list from persisted entries (newest first).
    pub fn load_history(&self, history: Rc<RefCell<History>>) {
        let entries: Vec<HistoryEntry> = history.borrow().entries().cloned().collect();
        for entry in entries.iter().rev() {
            self.history_list
                .append(&make_history_row(entry, &self.history_list, Rc::clone(&history)));
        }
    }

    pub fn present(&self) {
        self.window.present();
    }
}

/// Helper: locate the GDK monitor containing physical pixel point (x, y).
fn monitor_containing(x: i32, y: i32) -> Option<gtk4::gdk::Monitor> {
    let display = gtk4::gdk::Display::default()?;
    let monitors = display.monitors();
    for i in 0..monitors.n_items() {
        let obj = monitors.item(i)?;
        use glib::object::Cast;
        if let Ok(mon) = obj.downcast::<gtk4::gdk::Monitor>() {
            let geo = mon.geometry();
            let scale = mon.scale_factor().max(1);
            let px = geo.x() * scale;
            let py = geo.y() * scale;
            let pw = geo.width() * scale;
            let ph = geo.height() * scale;
            if x >= px && x < px + pw && y >= py && y < py + ph {
                return Some(mon);
            }
        }
    }
    None
}

fn cursor_position() -> Option<(i32, i32)> {
    let out = std::process::Command::new("xdotool")
        .args(["getmouselocation", "--shell"])
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout);
    let mut x: Option<i32> = None;
    let mut y: Option<i32> = None;
    for line in s.lines() {
        if let Some(v) = line.strip_prefix("X=") { x = v.parse().ok(); }
        else if let Some(v) = line.strip_prefix("Y=") { y = v.parse().ok(); }
    }
    x.zip(y)
}

fn window_xid(window: &ApplicationWindow) -> Option<u64> {
    use glib::object::Cast;
    window.surface().and_then(|s| s.downcast::<gdk4_x11::X11Surface>().ok()).map(|x| x.xid())
}

/// Use a saved position for the cursor's monitor if we have one; otherwise
/// fall back to centered-bottom on that monitor.
fn position_for_cursor(window: &ApplicationWindow, state: &Rc<RefCell<WindowState>>) {
    let Some((cx, cy)) = cursor_position() else { return };
    let Some(mon) = monitor_containing(cx, cy) else { return };
    let key = monitor_key(&mon);

    if let Some((x, y)) = state.borrow().get(&key) {
        if let Some(xid) = window_xid(window) {
            let _ = std::process::Command::new("xdotool")
                .args(["windowmove", &xid.to_string(), &x.to_string(), &y.to_string()])
                .status();
            return;
        }
    }
    position_center_bottom(window);
}

/// Read window position via xdotool and persist it keyed by the monitor the
/// window currently sits on.
fn save_window_position(window: &ApplicationWindow, state: &Rc<RefCell<WindowState>>) {
    let Some(xid) = window_xid(window) else { return };
    let Ok(out) = std::process::Command::new("xdotool")
        .args(["getwindowgeometry", "--shell", &xid.to_string()])
        .output()
    else { return };
    let s = String::from_utf8_lossy(&out.stdout);
    let mut x: Option<i32> = None;
    let mut y: Option<i32> = None;
    let mut w: Option<i32> = None;
    let mut h: Option<i32> = None;
    for line in s.lines() {
        if let Some(v) = line.strip_prefix("X=") { x = v.parse().ok(); }
        else if let Some(v) = line.strip_prefix("Y=") { y = v.parse().ok(); }
        else if let Some(v) = line.strip_prefix("WIDTH=") { w = v.parse().ok(); }
        else if let Some(v) = line.strip_prefix("HEIGHT=") { h = v.parse().ok(); }
    }
    let (Some(x), Some(y), Some(w), Some(h)) = (x, y, w, h) else { return };
    let center = (x + w / 2, y + h / 2);
    let Some(mon) = monitor_containing(center.0, center.1) else { return };
    let mut st = state.borrow_mut();
    st.set(monitor_key(&mon), (x, y));
    st.save();
}

fn position_center_bottom(window: &ApplicationWindow) {
    // Parse cursor position from xdotool (X11 physical pixels).
    let cursor_pos: Option<(i32, i32)> = std::process::Command::new("xdotool")
        .args(["getmouselocation", "--shell"])
        .output()
        .ok()
        .and_then(|out| {
            let s = String::from_utf8_lossy(&out.stdout);
            let mut x = None;
            let mut y = None;
            for line in s.lines() {
                if let Some(v) = line.strip_prefix("X=") {
                    x = v.parse::<i32>().ok();
                } else if let Some(v) = line.strip_prefix("Y=") {
                    y = v.parse::<i32>().ok();
                }
            }
            x.zip(y)
        });

    let (cursor_x, cursor_y) = match cursor_pos {
        Some(p) => p,
        None => return,
    };

    // Find the GDK monitor containing the cursor.
    // GDK geometry is in logical (scaled) pixels; multiply by scale_factor for physical.
    let display = match gtk4::gdk::Display::default() {
        Some(d) => d,
        None => return,
    };
    let monitors = display.monitors();
    let mut target: Option<(i32, i32, i32, i32)> = None; // (phys_x, phys_y, phys_w, phys_h)
    for i in 0..monitors.n_items() {
        if let Some(obj) = monitors.item(i) {
            use glib::object::Cast;
            if let Ok(mon) = obj.downcast::<gtk4::gdk::Monitor>() {
                let geo = mon.geometry();
                let scale = mon.scale_factor().max(1) as i32;
                let px = geo.x() * scale;
                let py = geo.y() * scale;
                let pw = geo.width() * scale;
                let ph = geo.height() * scale;
                if cursor_x >= px && cursor_x < px + pw && cursor_y >= py && cursor_y < py + ph {
                    target = Some((px, py, pw, ph));
                    break;
                }
            }
        }
    }
    let (mon_px, mon_py, mon_pw, mon_ph) = match target {
        Some(t) => t,
        None => return,
    };

    // Window size in physical pixels. window.width()/height() return 0 before the first
    // frame is drawn, so fall back to the default size we set in new().
    let scale = window.scale_factor().max(1) as i32;
    let (default_w, default_h) = window.default_size();
    let logical_w = if window.width() > 10 { window.width() } else if default_w > 0 { default_w } else { 480 };
    let logical_h = if window.height() > 10 { window.height() } else if default_h > 0 { default_h } else { 520 };
    let win_w = logical_w * scale;
    let win_h = logical_h * scale;
    let margin = 40 * scale;

    let target_x = (mon_px + (mon_pw - win_w) / 2).max(mon_px);
    let target_y = (mon_py + mon_ph - win_h - margin).max(mon_py);

    // Move via xdotool using the X11 window ID.
    let xid: Option<u64> = window.surface().and_then(|s| {
        use glib::object::Cast;
        s.downcast::<gdk4_x11::X11Surface>().ok().map(|x11| x11.xid())
    });
    if let Some(xid) = xid {
        let _ = std::process::Command::new("xdotool")
            .args(["windowmove", &xid.to_string(), &target_x.to_string(), &target_y.to_string()])
            .status();
    }
}

/// Join two text segments with a space if neither already has one at the boundary.
pub(crate) fn space_join(existing: &str, seg: &str) -> String {
    if !existing.is_empty() && !existing.ends_with(' ') && !seg.starts_with(' ') {
        format!(" {seg}")
    } else {
        seg.to_string()
    }
}

fn make_history_row(
    entry: &HistoryEntry,
    list: &ListBox,
    history: Rc<RefCell<History>>,
) -> ListBoxRow {
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

    let del_btn = Button::with_label("🗑");
    del_btn.set_valign(gtk4::Align::Start);
    {
        let timestamp = entry.timestamp.clone();
        let row_ref = row.clone();
        let list_ref = list.clone();
        del_btn.connect_clicked(move |_| {
            history.borrow_mut().remove_by_timestamp(&timestamp);
            list_ref.remove(&row_ref);
        });
    }

    hbox.append(&time_lbl);
    hbox.append(&text_lbl);
    hbox.append(&copy_btn);
    hbox.append(&del_btn);
    row.set_child(Some(&hbox));
    row
}
