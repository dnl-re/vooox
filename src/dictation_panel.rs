use crate::audio;
use crate::config::{Config, PanelMode};
use crate::history::{History, HistoryEntry};
use crate::tray::{AppCommand, WHISPER_MODELS};
use crate::window_state::{monitor_key, WindowState};
use crate::x11_window;
use crossbeam_channel::Sender;
use std::collections::VecDeque;
use std::rc::Rc;
use std::cell::{Cell, RefCell};
use glib;
use gtk4::prelude::*;
use gtk4::{
    gio, Application, ApplicationWindow, Box as GtkBox, CssProvider, DrawingArea, Label, LevelBar,
    MenuButton, Orientation, ScrolledWindow, Separator, TextView,
};

const CSS: &str = r#"
.status-rec  { color: #ff4444; font-weight: bold; }
.status-ptt  { color: #c93cff; font-weight: bold; }
.status-proc { color: #ffaa00; font-weight: bold; }
.status-idle { color: #888888; }
.copy-btn-done { background-color: #26a269; color: white; }
.history-time  { font-size: 11px; color: #888888; }
.toast { color: #26a269; font-size: 14px; }
window.dictation-window { background: transparent; }
.panel-root {
    border-radius: 12px;
    border: 1px solid alpha(currentColor, 0.12);
}

/* ── icon mode (pill) ───────────────────────────────────────────── */
.panel-pill {
    background-color: rgba(20, 20, 20, 0.95);
    border-radius: 20px;
    padding: 6px 14px;
    border: 1px solid rgba(255, 255, 255, 0.08);
    color: #dddddd;
}
.pill-dot {
    color: #ff4444;
    font-size: 16px;
    font-weight: bold;
    animation: pill-pulse 1s ease-in-out infinite;
}
@keyframes pill-pulse {
    0%   { opacity: 0.35; }
    50%  { opacity: 1.0;  }
    100% { opacity: 0.35; }
}
.pill-dot-ptt {
    color: #c93cff;
}
.pill-dot-proc {
    color: #ffaa00;
    animation: none;
    opacity: 1.0;
}
.pill-dot-done {
    color: #26a269;
    animation: none;
    opacity: 1.0;
}
.pill-timer {
    color: #cccccc;
    font-size: 12px;
    font-family: monospace;
}
"#;

const PILL_W: i32 = 150;
const PILL_H: i32 = 40;
const WIN_W: i32 = 480;
const WIN_H: i32 = 260;
const WAVE_BARS: usize = 14;
const WAVE_W: i32 = 80;
const WAVE_H: i32 = 22;

pub struct DictationPanel {
    window: ApplicationWindow,
    // window-mode children
    window_layout: GtkBox,
    status_label: Label,
    timer_label: Label,
    level_bar: LevelBar,
    text_view: TextView,
    toast_label: Label,
    // icon-mode (pill) children
    pill_layout: GtkBox,
    pill_dot: Label,
    pill_waveform: DrawingArea,
    pill_phase: Rc<Cell<PillPhase>>,
    pill_timer: Label,
    // shared state
    level_meter: Rc<RefCell<Option<audio::LevelMeter>>>,
    level_history: Rc<RefCell<VecDeque<f32>>>,
    timer_source: Rc<RefCell<Option<glib::SourceId>>>,
    timer_seconds: Rc<RefCell<u32>>,
    base_text: Rc<RefCell<String>>,
    win_state: Rc<RefCell<WindowState>>,
    mode: Rc<Cell<PanelMode>>,
    mode_action: gio::SimpleAction,
    processing_started_at: Rc<Cell<Option<std::time::Instant>>>,
}

impl DictationPanel {
    pub fn new(
        app: &Application,
        cmd_tx: Sender<AppCommand>,
        config: Rc<RefCell<Config>>,
    ) -> Self {
        install_css();
        let initial_mode = config.borrow().panel_mode;

        // ── widgets ───────────────────────────────────────────────────────
        let status_label = build_status_label();
        let timer_label = build_timer_label();
        let level_bar = build_level_bar();
        let menu_btn = build_menu_button();
        let header_box = build_header_box(&status_label, &timer_label, &level_bar, &menu_btn);

        let text_view = build_text_view();
        let text_scroll = ScrolledWindow::builder()
            .vexpand(true)
            .min_content_height(80)
            .build();
        text_scroll.set_child(Some(&text_view));

        let toast_label = build_toast_label();
        let window_layout = build_window_layout(&header_box, &text_scroll, &toast_label);

        let level_history: Rc<RefCell<VecDeque<f32>>> = Rc::new(RefCell::new(
            std::iter::repeat(0.0).take(WAVE_BARS).collect(),
        ));
        let pill_phase: Rc<Cell<PillPhase>> = Rc::new(Cell::new(PillPhase::Recording));
        let waveform = build_waveform_area(Rc::clone(&level_history), Rc::clone(&pill_phase));
        let pill_dot = build_pill_dot();
        let pill_timer = build_pill_timer();
        let pill_layout = build_pill_layout(&pill_dot, &waveform, &pill_timer);

        // ── window ────────────────────────────────────────────────────────
        let outer = GtkBox::new(Orientation::Vertical, 0);
        outer.append(&window_layout);
        outer.append(&pill_layout);
        let window = build_window(app, initial_mode, &outer);
        set_initial_layout_visibility(initial_mode, &window_layout, &pill_layout);

        // ── actions, gestures, lifecycle ──────────────────────────────────
        let mode_action = wire_kebab_actions(&window, &cmd_tx, &config, initial_mode);
        let win_state = Rc::new(RefCell::new(WindowState::load()));
        wire_close_request(&window, Rc::clone(&win_state));
        wire_drag_gestures(&window, &header_box, &menu_btn, &pill_layout);

        DictationPanel {
            window,
            window_layout,
            status_label,
            timer_label,
            level_bar,
            text_view,
            toast_label,
            pill_layout,
            pill_dot,
            pill_waveform: waveform,
            pill_phase,
            pill_timer,
            level_meter: Rc::new(RefCell::new(None)),
            level_history,
            timer_source: Rc::new(RefCell::new(None)),
            timer_seconds: Rc::new(RefCell::new(0)),
            base_text: Rc::new(RefCell::new(String::new())),
            win_state,
            mode: Rc::new(Cell::new(initial_mode)),
            mode_action,
            processing_started_at: Rc::new(Cell::new(None)),
        }
    }

    /// Switch between Window and Icon modes. Idempotent.
    pub fn apply_mode(&self, mode: PanelMode) {
        if self.mode.get() == mode {
            return;
        }
        self.mode.set(mode);
        self.mode_action.set_state(&mode.as_str().to_variant());

        match mode {
            PanelMode::Window => {
                self.pill_layout.set_visible(false);
                self.window_layout.set_visible(true);
                self.window.set_default_size(WIN_W, WIN_H);
            }
            PanelMode::Icon => {
                self.window_layout.set_visible(false);
                self.pill_layout.set_visible(true);
                self.window.set_default_size(PILL_W, PILL_H);
                self.set_pill_dot_state(PillDot::Recording);
                self.pill_phase.set(PillPhase::Recording);
            }
        }
    }

    fn set_pill_dot_state(&self, state: PillDot) {
        self.pill_dot.remove_css_class("pill-dot-proc");
        self.pill_dot.remove_css_class("pill-dot-done");
        self.pill_dot.remove_css_class("pill-dot-ptt");
        match state {
            PillDot::Recording => { /* base .pill-dot class only — pulses red */ }
            PillDot::Processing => self.pill_dot.add_css_class("pill-dot-proc"),
            PillDot::Done => self.pill_dot.add_css_class("pill-dot-done"),
        }
    }

    /// Switch recording visuals between toggle-mode (red) and push-to-talk
    /// (purple). Safe to call repeatedly. Only takes effect while in the
    /// recording phase — processing/done visuals are not touched.
    pub fn set_ptt_active(&self, active: bool) {
        if active {
            self.status_label.set_text("● Push-to-Talk");
            self.status_label.remove_css_class("status-rec");
            self.status_label.add_css_class("status-ptt");
            self.pill_dot.add_css_class("pill-dot-ptt");
        } else {
            self.status_label.set_text("● Aufnahme");
            self.status_label.remove_css_class("status-ptt");
            self.status_label.add_css_class("status-rec");
            self.pill_dot.remove_css_class("pill-dot-ptt");
        }
    }

    pub fn show_recording(&self, device: &cpal::Device) {
        if let Some(id) = self.timer_source.borrow_mut().take() {
            id.remove();
        }

        // Window-mode text-view append/replace logic
        if self.mode.get() == PanelMode::Window {
            if self.text_view.has_focus() {
                let buf = self.text_view.buffer();
                let mut existing: String =
                    buf.text(&buf.start_iter(), &buf.end_iter(), false).into();
                if !existing.is_empty() && !existing.ends_with(' ') {
                    existing.push(' ');
                    buf.insert(&mut buf.end_iter(), " ");
                }
                *self.base_text.borrow_mut() = existing;
            } else {
                self.text_view.buffer().set_text("");
                *self.base_text.borrow_mut() = String::new();
            }
        } else {
            // Icon mode: text never shown here, base is empty so set_transcript works
            self.text_view.buffer().set_text("");
            *self.base_text.borrow_mut() = String::new();
        }

        // window-mode status
        self.status_label.set_text("● Aufnahme");
        self.status_label.remove_css_class("status-proc");
        self.status_label.remove_css_class("status-idle");
        self.status_label.remove_css_class("status-ptt");
        self.status_label.add_css_class("status-rec");

        // icon-mode visuals
        self.set_pill_dot_state(PillDot::Recording);
        self.pill_phase.set(PillPhase::Recording);
        // reset waveform history
        {
            let mut hist = self.level_history.borrow_mut();
            for v in hist.iter_mut() {
                *v = 0.0;
            }
        }
        self.pill_timer.set_text("00:00");

        // start level meter — drives BOTH the LevelBar (window mode)
        // and the DrawingArea history (icon mode)
        *self.level_meter.borrow_mut() = None;
        match audio::LevelMeter::start(device) {
            Ok(meter) => {
                *self.level_meter.borrow_mut() = Some(meter);

                // 50 ms: update the window-mode LevelBar.
                let meter_rc = Rc::clone(&self.level_meter);
                let bar = self.level_bar.clone();
                glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
                    match meter_rc.borrow().as_ref() {
                        Some(m) => {
                            bar.set_value(m.get() as f64);
                            glib::ControlFlow::Continue
                        }
                        None => {
                            bar.set_value(0.0);
                            glib::ControlFlow::Break
                        }
                    }
                });

                // 70 ms: shift the waveform history and redraw the pill.
                let meter_rc = Rc::clone(&self.level_meter);
                let hist = Rc::clone(&self.level_history);
                let area = self.pill_waveform.clone();
                glib::timeout_add_local(std::time::Duration::from_millis(70), move || {
                    let Some(level) = meter_rc.borrow().as_ref().map(|m| m.get()) else {
                        return glib::ControlFlow::Break;
                    };
                    let mut h = hist.borrow_mut();
                    if h.len() == WAVE_BARS {
                        h.pop_front();
                    }
                    h.push_back(level);
                    drop(h);
                    area.queue_draw();
                    glib::ControlFlow::Continue
                });
            }
            Err(e) => eprintln!("[panel] level meter: {e}"),
        }

        // 1-second timer (both modes)
        *self.timer_seconds.borrow_mut() = 0;
        self.timer_label.set_text("00:00");
        self.pill_timer.set_text("00:00");
        let secs_rc = Rc::clone(&self.timer_seconds);
        let lbl = self.timer_label.clone();
        let pill_lbl = self.pill_timer.clone();
        let id = glib::timeout_add_local(std::time::Duration::from_secs(1), move || {
            let mut s = secs_rc.borrow_mut();
            *s += 1;
            let txt = format!("{:02}:{:02}", *s / 60, *s % 60);
            lbl.set_text(&txt);
            pill_lbl.set_text(&txt);
            glib::ControlFlow::Continue
        });
        *self.timer_source.borrow_mut() = Some(id);

        // Remember the previously active window so we can hand keyboard focus
        // back to it after present() steals it for a moment.
        let prev_active = x11_window::active_window_id();

        gtk4::prelude::GtkWindowExt::set_focus(&self.window, None::<&gtk4::Widget>);
        self.window.present();

        let win = self.window.clone();
        glib::timeout_add_local_once(std::time::Duration::from_millis(50), move || {
            position_next_to_cursor(&win);
            let our_xid = x11_window::window_xid(&win);
            if let Some(xid) = our_xid {
                x11_window::activate_window(xid);
            }
            if let Some(ref xid) = prev_active {
                x11_window::focus_window(xid);
            }
            if let Some(xid) = our_xid {
                x11_window::raise_window(xid);
            }
        });
    }

    pub fn show_processing(&self) {
        *self.level_meter.borrow_mut() = None;
        if let Some(id) = self.timer_source.borrow_mut().take() {
            id.remove();
        }
        self.status_label.set_text("⏳ Verarbeitung…");
        self.status_label.remove_css_class("status-rec");
        self.status_label.remove_css_class("status-idle");
        self.status_label.remove_css_class("status-ptt");
        self.status_label.add_css_class("status-proc");

        // icon-mode: switch waveform to traveling sine wave in orange.
        self.set_pill_dot_state(PillDot::Processing);
        self.pill_phase.set(PillPhase::Processing);
        self.processing_started_at.set(Some(std::time::Instant::now()));
        self.start_processing_animation();
    }

    /// Drive the pill waveform with a traveling sine wave during processing.
    fn start_processing_animation(&self) {
        let hist = Rc::clone(&self.level_history);
        let area = self.pill_waveform.clone();
        let phase = Rc::clone(&self.pill_phase);
        let start = std::time::Instant::now();
        glib::timeout_add_local(std::time::Duration::from_millis(40), move || {
            if phase.get() != PillPhase::Processing {
                return glib::ControlFlow::Break;
            }
            let t = start.elapsed().as_secs_f32();
            {
                let mut h = hist.borrow_mut();
                let count = h.len();
                for (i, slot) in h.iter_mut().enumerate() {
                    // sine wave moving left→right, normalized to 0..1
                    let phase_offset = i as f32 / count.max(1) as f32 * std::f32::consts::TAU;
                    let v = (t * 5.5 - phase_offset).sin() * 0.5 + 0.5;
                    // shape into draw_func's expected linear-level scale —
                    // pre-square so sqrt() in draw_func produces a clean curve
                    let amp = 0.18 + 0.62 * v; // ~0.18..0.80
                    *slot = amp * amp;
                }
            }
            area.queue_draw();
            glib::ControlFlow::Continue
        });
    }

    pub fn text_view_text(&self) -> String {
        let buf = self.text_view.buffer();
        buf.text(&buf.start_iter(), &buf.end_iter(), false).into()
    }

    pub fn set_transcript(&self, text: &str) {
        let base = self.base_text.borrow();
        let full = format!("{}{}", *base, text);
        self.text_view.buffer().set_text(&full);
    }

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

    pub fn finish(&self, full_text: &str, cfg: &Config, history: Rc<RefCell<History>>) {
        self.status_label.set_text("○ Bereit");
        self.status_label.remove_css_class("status-proc");
        self.status_label.remove_css_class("status-rec");
        self.status_label.remove_css_class("status-ptt");
        self.status_label.add_css_class("status-idle");
        self.timer_label.set_text("");

        // Keep the processing animation visible for at least 600 ms so the
        // user actually sees it even if whisper returns very quickly.
        let min_processing = std::time::Duration::from_millis(600);
        let remaining = self
            .processing_started_at
            .get()
            .and_then(|t| min_processing.checked_sub(t.elapsed()))
            .unwrap_or(std::time::Duration::ZERO);

        if full_text.is_empty() {
            if self.mode.get() == PanelMode::Icon {
                let panel_window = self.window.clone();
                let win_state = Rc::clone(&self.win_state);
                let phase = Rc::clone(&self.pill_phase);
                let dot = self.pill_dot.clone();
                glib::timeout_add_local_once(remaining, move || {
                    phase.set(PillPhase::Recording);
                    dot.remove_css_class("pill-dot-proc");
                    dot.remove_css_class("pill-dot-done");
                    save_window_position(&panel_window, &win_state);
                    panel_window.hide();
                });
            }
            return;
        }

        if let Some(display) = gtk4::gdk::Display::default() {
            display.clipboard().set_text(full_text);
        }

        self.toast_label.set_text("✓ In Zwischenablage kopiert");
        let lbl = self.toast_label.clone();
        glib::timeout_add_local_once(std::time::Duration::from_secs(3), move || {
            lbl.set_text("");
        });

        // icon-mode: hold processing animation for min duration, then play
        // the green-sweep "done" animation, which hides the pill at its end.
        if self.mode.get() == PanelMode::Icon {
            let dot = self.pill_dot.clone();
            let phase = Rc::clone(&self.pill_phase);
            let timer_lbl = self.pill_timer.clone();
            let hist = Rc::clone(&self.level_history);
            let area = self.pill_waveform.clone();
            let panel_window = self.window.clone();
            let win_state = Rc::clone(&self.win_state);
            glib::timeout_add_local_once(remaining, move || {
                // Stop the processing animation timer.
                phase.set(PillPhase::Done);
                dot.remove_css_class("pill-dot-proc");
                dot.add_css_class("pill-dot-done");
                timer_lbl.set_text("");
                start_done_animation(hist, area, phase, dot, panel_window, win_state);
            });
        }

        let entry = HistoryEntry {
            text: full_text.to_string(),
            timestamp: crate::history::now_rfc3339(),
            model: cfg.model.clone(),
            language: cfg.language.clone(),
        };
        history.borrow_mut().push(entry);
    }

    pub fn present(&self) {
        self.window.present();
    }

    pub fn hide(&self) {
        save_window_position(&self.window, &self.win_state);
        self.window.hide();
    }
}

#[derive(Clone, Copy)]
enum PillDot {
    Recording,
    Processing,
    Done,
}

/// Green "done" sweep across the pill waveform:
/// a single bright peak travels left→right, leaving a settled trail
/// behind it, then everything fades to zero and the pill hides.
fn start_done_animation(
    hist: Rc<RefCell<VecDeque<f32>>>,
    area: DrawingArea,
    phase: Rc<Cell<PillPhase>>,
    dot: Label,
    win: ApplicationWindow,
    win_state: Rc<RefCell<WindowState>>,
) {
    let start = std::time::Instant::now();
    let sweep_dur = 0.55_f32;
    let fade_dur = 0.45_f32;
    let total_dur = sweep_dur + fade_dur;

    glib::timeout_add_local(std::time::Duration::from_millis(35), move || {
        if phase.get() != PillPhase::Done {
            return glib::ControlFlow::Break;
        }
        let t = start.elapsed().as_secs_f32();
        if t >= total_dur {
            save_window_position(&win, &win_state);
            win.hide();
            dot.remove_css_class("pill-dot-done");
            phase.set(PillPhase::Recording);
            area.queue_draw();
            return glib::ControlFlow::Break;
        }

        let n = {
            let h = hist.borrow();
            h.len().max(1) as f32
        };
        let mut h = hist.borrow_mut();
        for (i, slot) in h.iter_mut().enumerate() {
            let i_f = i as f32;
            let v: f32 = if t < sweep_dur {
                let progress = t / sweep_dur * n;
                // Triangle pulse around `progress`, width ~1.4 bars.
                let dist = (progress - i_f).abs();
                let pulse = (1.0_f32 - dist / 1.4).max(0.0);
                // Bars already passed by the sweep settle at ~0.45.
                let settled: f32 = if progress > i_f + 0.4 { 0.45 } else { 0.0 };
                pulse.max(settled)
            } else {
                let fade_t = ((t - sweep_dur) / fade_dur).clamp(0.0, 1.0);
                0.45 * (1.0 - fade_t)
            };
            // draw_func applies sqrt(); pre-square so the on-screen height
            // tracks `v` linearly.
            *slot = v * v;
        }
        drop(h);
        area.queue_draw();
        glib::ControlFlow::Continue
    });
}

#[derive(Clone, Copy, PartialEq)]
enum PillPhase {
    Recording,
    Processing,
    Done,
}

fn begin_move_from_gesture(win: &ApplicationWindow, gesture: &gtk4::GestureClick, x: f64, y: f64) {
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
}

/// Place the window directly next to the mouse cursor: to the right by
/// default, flipping to the left if the right side would overflow the
/// monitor edge. Y is centered on the cursor and clamped into the monitor.
fn position_next_to_cursor(window: &ApplicationWindow) {
    let Some((cx, cy)) = x11_window::cursor_position() else { return };
    let Some(mon) = x11_window::monitor_containing(cx, cy) else { return };
    let Some(xid) = x11_window::window_xid(window) else { return };

    let (mon_x, mon_y, mon_w, mon_h) = x11_window::monitor_geometry_physical(&mon);
    let scale = window.scale_factor().max(1);
    let (default_w, default_h) = window.default_size();
    let logical_w = if window.width() > 10 {
        window.width()
    } else if default_w > 0 {
        default_w
    } else {
        WIN_W
    };
    let logical_h = if window.height() > 10 {
        window.height()
    } else if default_h > 0 {
        default_h
    } else {
        WIN_H
    };
    let win_w = logical_w * scale;
    let win_h = logical_h * scale;
    let margin = 16 * scale;

    let right_x = cx + margin;
    let x = if right_x + win_w <= mon_x + mon_w {
        right_x
    } else {
        (cx - margin - win_w).max(mon_x)
    };
    let y = (cy - win_h / 2)
        .max(mon_y)
        .min(mon_y + mon_h - win_h);

    x11_window::move_window(xid, x, y);
}

fn save_window_position(window: &ApplicationWindow, state: &Rc<RefCell<WindowState>>) {
    let Some(xid) = x11_window::window_xid(window) else { return };
    let Some((x, y, w, h)) = x11_window::window_geometry(xid) else { return };
    let center = (x + w / 2, y + h / 2);
    let Some(mon) = x11_window::monitor_containing(center.0, center.1) else { return };
    let mut st = state.borrow_mut();
    st.set(monitor_key(&mon), (x, y));
    st.save();
}

pub(crate) fn space_join(existing: &str, seg: &str) -> String {
    if !existing.is_empty() && !existing.ends_with(' ') && !seg.starts_with(' ') {
        format!(" {seg}")
    } else {
        seg.to_string()
    }
}

// ─── widget builders ────────────────────────────────────────────────────────

fn install_css() {
    let provider = CssProvider::new();
    provider.load_from_data(CSS);
    if let Some(display) = gtk4::gdk::Display::default() {
        gtk4::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

fn build_status_label() -> Label {
    let lbl = Label::new(Some("○ Bereit"));
    lbl.add_css_class("status-idle");
    lbl
}

fn build_timer_label() -> Label {
    let lbl = Label::new(Some(""));
    lbl.set_hexpand(true);
    lbl.set_xalign(1.0);
    lbl
}

fn build_level_bar() -> LevelBar {
    let bar = LevelBar::new();
    bar.set_min_value(0.0);
    bar.set_max_value(1.0);
    bar.set_size_request(100, -1);
    bar.set_valign(gtk4::Align::Center);
    bar
}

fn build_menu_button() -> MenuButton {
    let menu_model = build_menu_model();
    let btn = MenuButton::builder()
        .icon_name("view-more-symbolic")
        .menu_model(&menu_model)
        .valign(gtk4::Align::Center)
        .build();
    btn.add_css_class("flat");
    btn
}

fn build_menu_model() -> gio::Menu {
    let menu = gio::Menu::new();
    menu.append(Some("Verlauf"), Some("panel.history"));
    menu.append(Some("Einstellungen"), Some("panel.settings"));

    let models = gio::Menu::new();
    for m in WHISPER_MODELS {
        let item = gio::MenuItem::new(Some(m), None);
        item.set_action_and_target_value(Some("panel.model"), Some(&m.to_variant()));
        models.append_item(&item);
    }
    menu.append_section(Some("Modell"), &models);

    let modes = gio::Menu::new();
    for (label, value) in [("Diktierfenster", "window"), ("Nur Icon", "icon")] {
        let item = gio::MenuItem::new(Some(label), None);
        item.set_action_and_target_value(Some("panel.mode"), Some(&value.to_variant()));
        modes.append_item(&item);
    }
    menu.append_section(Some("Modus"), &modes);

    let actions = gio::Menu::new();
    actions.append(Some("Fenster schließen"), Some("panel.close"));
    actions.append(Some("App beenden"), Some("panel.quit"));
    menu.append_section(None, &actions);

    menu
}

fn build_header_box(
    status: &Label,
    timer: &Label,
    level: &LevelBar,
    menu_btn: &MenuButton,
) -> GtkBox {
    let header = GtkBox::new(Orientation::Horizontal, 8);
    header.set_margin_top(8);
    header.set_margin_bottom(8);
    header.set_margin_start(12);
    header.set_margin_end(12);
    header.append(status);
    header.append(timer);
    header.append(level);
    header.append(menu_btn);
    header
}

fn build_text_view() -> TextView {
    let tv = TextView::new();
    tv.set_editable(true);
    tv.set_wrap_mode(gtk4::WrapMode::WordChar);
    tv.set_left_margin(12);
    tv.set_right_margin(12);
    tv.set_top_margin(8);
    tv.set_bottom_margin(8);
    tv
}

fn build_toast_label() -> Label {
    let lbl = Label::new(None);
    lbl.add_css_class("toast");
    lbl.set_hexpand(true);
    lbl.set_xalign(0.5);
    lbl.set_margin_top(4);
    lbl.set_margin_bottom(4);
    lbl
}

fn build_window_layout(header: &GtkBox, scroll: &ScrolledWindow, toast: &Label) -> GtkBox {
    let layout = GtkBox::new(Orientation::Vertical, 0);
    layout.add_css_class("background");
    layout.add_css_class("panel-root");
    layout.append(header);
    layout.append(&Separator::new(Orientation::Horizontal));
    layout.append(scroll);
    layout.append(toast);
    layout
}

fn build_pill_dot() -> Label {
    let dot = Label::new(Some("●"));
    dot.add_css_class("pill-dot");
    dot.set_valign(gtk4::Align::Center);
    dot
}

fn build_pill_timer() -> Label {
    let lbl = Label::new(Some("00:00"));
    lbl.add_css_class("pill-timer");
    lbl.set_valign(gtk4::Align::Center);
    lbl
}

fn build_pill_layout(dot: &Label, waveform: &DrawingArea, timer: &Label) -> GtkBox {
    let layout = GtkBox::new(Orientation::Horizontal, 10);
    layout.add_css_class("panel-pill");
    layout.set_valign(gtk4::Align::Center);
    layout.set_halign(gtk4::Align::Center);
    layout.append(dot);
    layout.append(waveform);
    layout.append(timer);
    layout
}

fn build_waveform_area(
    history: Rc<RefCell<VecDeque<f32>>>,
    phase: Rc<Cell<PillPhase>>,
) -> DrawingArea {
    let area = DrawingArea::new();
    area.set_content_width(WAVE_W);
    area.set_content_height(WAVE_H);
    area.set_size_request(WAVE_W, WAVE_H);
    area.set_valign(gtk4::Align::Center);
    area.set_draw_func(move |_, cr, w, h| {
        let hist = history.borrow();
        let n = hist.len().max(1);
        let bar_w = (w as f64 / n as f64) * 0.55;
        let gap = (w as f64 / n as f64) - bar_w;
        let center_y = h as f64 / 2.0;
        match phase.get() {
            PillPhase::Recording => cr.set_source_rgba(1.0, 0.32, 0.32, 0.95),
            PillPhase::Processing => cr.set_source_rgba(1.0, 0.68, 0.18, 0.95),
            PillPhase::Done => cr.set_source_rgba(0.15, 0.75, 0.40, 0.95),
        }
        for (i, &lvl) in hist.iter().enumerate() {
            // Audio levels are logarithmic — sqrt() lifts quiet speech into
            // a visible range while still letting peaks ride high.
            let l = lvl.clamp(0.0, 1.0) as f64;
            let scaled = (l.sqrt() * 2.2).min(1.0);
            let bar_h = (scaled * h as f64 * 0.9).max(2.0);
            let x = i as f64 * (bar_w + gap) + gap * 0.5;
            let y = center_y - bar_h / 2.0;
            let r = (bar_w / 2.0).min(bar_h / 2.0);
            cr.move_to(x + r, y);
            cr.line_to(x + bar_w - r, y);
            cr.arc(x + bar_w - r, y + r, r, -std::f64::consts::FRAC_PI_2, 0.0);
            cr.line_to(x + bar_w, y + bar_h - r);
            cr.arc(x + bar_w - r, y + bar_h - r, r, 0.0, std::f64::consts::FRAC_PI_2);
            cr.line_to(x + r, y + bar_h);
            cr.arc(x + r, y + bar_h - r, r, std::f64::consts::FRAC_PI_2, std::f64::consts::PI);
            cr.line_to(x, y + r);
            cr.arc(x + r, y + r, r, std::f64::consts::PI, std::f64::consts::PI * 1.5);
            cr.close_path();
            let _ = cr.fill();
        }
    });
    area
}

fn build_window(app: &Application, mode: PanelMode, child: &GtkBox) -> ApplicationWindow {
    let (w, h) = if mode == PanelMode::Icon { (PILL_W, PILL_H) } else { (WIN_W, WIN_H) };
    let win = ApplicationWindow::builder()
        .application(app)
        .title("vooox")
        .default_width(w)
        .default_height(h)
        .decorated(false)
        .build();
    win.add_css_class("dictation-window");
    win.set_child(Some(child));
    win
}

fn set_initial_layout_visibility(mode: PanelMode, window_layout: &GtkBox, pill_layout: &GtkBox) {
    let (window_visible, pill_visible) = match mode {
        PanelMode::Window => (true, false),
        PanelMode::Icon => (false, true),
    };
    window_layout.set_visible(window_visible);
    pill_layout.set_visible(pill_visible);
}

// ─── action wiring ──────────────────────────────────────────────────────────

/// Wires the kebab-menu's `panel.*` actions to outgoing AppCommands.
/// Returns the stateful mode-action so the panel can keep its radio state
/// in sync when the mode is changed via the tray instead of the kebab.
fn wire_kebab_actions(
    window: &ApplicationWindow,
    cmd_tx: &Sender<AppCommand>,
    config: &Rc<RefCell<Config>>,
    initial_mode: PanelMode,
) -> gio::SimpleAction {
    let action_group = gio::SimpleActionGroup::new();

    for (name, cmd) in [
        ("history", AppCommand::OpenHistory),
        ("settings", AppCommand::OpenSettings),
        ("close", AppCommand::HidePanel),
        ("quit", AppCommand::Quit),
    ] {
        let action = gio::SimpleAction::new(name, None);
        let tx = cmd_tx.clone();
        let cmd_clone = cmd.clone();
        action.connect_activate(move |_, _| { let _ = tx.send(cmd_clone.clone()); });
        action_group.add_action(&action);
    }

    let model_action = gio::SimpleAction::new_stateful(
        "model",
        Some(glib::VariantTy::STRING),
        &config.borrow().model.to_variant(),
    );
    let tx = cmd_tx.clone();
    model_action.connect_activate(move |action, param| {
        if let Some(s) = param.and_then(|p| p.get::<String>()) {
            action.set_state(&s.to_variant());
            let _ = tx.send(AppCommand::SetModel(s));
        }
    });
    action_group.add_action(&model_action);

    let mode_action = gio::SimpleAction::new_stateful(
        "mode",
        Some(glib::VariantTy::STRING),
        &initial_mode.as_str().to_variant(),
    );
    let tx = cmd_tx.clone();
    mode_action.connect_activate(move |action, param| {
        if let Some(s) = param.and_then(|p| p.get::<String>()) {
            if let Some(m) = PanelMode::from_str(&s) {
                action.set_state(&s.to_variant());
                let _ = tx.send(AppCommand::SetPanelMode(m));
            }
        }
    });
    action_group.add_action(&mode_action);

    window.insert_action_group("panel", Some(&action_group));
    mode_action
}

fn wire_close_request(window: &ApplicationWindow, state: Rc<RefCell<WindowState>>) {
    window.connect_close_request(move |win| {
        save_window_position(win, &state);
        win.hide();
        glib::Propagation::Stop
    });
}

/// Attaches drag-to-move gestures: clicking the window-mode header (except
/// on the kebab button) or anywhere on the pill body begins a window move.
fn wire_drag_gestures(
    window: &ApplicationWindow,
    header_box: &GtkBox,
    menu_btn: &MenuButton,
    pill_layout: &GtkBox,
) {
    let win = window.clone();
    let header = header_box.clone();
    let menu_btn_for_drag = menu_btn.clone();
    let drag = gtk4::GestureClick::new();
    drag.set_button(1);
    drag.connect_pressed(move |gesture, _n, x, y| {
        if click_landed_on(&header, &menu_btn_for_drag, x, y) {
            return;
        }
        begin_move_from_gesture(&win, gesture, x, y);
    });
    header_box.add_controller(drag);

    let win = window.clone();
    let drag = gtk4::GestureClick::new();
    drag.set_button(1);
    drag.connect_pressed(move |gesture, _n, x, y| {
        begin_move_from_gesture(&win, gesture, x, y);
    });
    pill_layout.add_controller(drag);
}

/// True when a click at (x, y) on `header` lands on `target` or one of its
/// descendants. Used to keep the header-drag gesture from swallowing clicks
/// on the kebab menu button.
fn click_landed_on(header: &GtkBox, target: &MenuButton, x: f64, y: f64) -> bool {
    let Some(picked) = header.pick(x, y, gtk4::PickFlags::DEFAULT) else { return false };
    let mut w = Some(picked);
    while let Some(cur) = w {
        if cur.eq(target) {
            return true;
        }
        w = cur.parent();
        if let Some(ref p) = w {
            if p.eq(header) {
                return false;
            }
        }
    }
    false
}
