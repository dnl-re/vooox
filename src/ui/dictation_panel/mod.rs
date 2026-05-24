mod build;
mod helpers;
mod wiring;

use self::build::*;
use self::helpers::{
    begin_move_from_gesture, position_next_to_cursor, save_transcription_to_history,
    save_window_position, schedule_auto_paste, start_done_animation,
};
use self::wiring::{wire_close_request, wire_drag_gestures, wire_kebab_actions};

use crate::audio;
use crate::storage::config::{Config, PanelMode};
use crate::storage::history::History;
use crate::storage::window_state::WindowState;
use crate::system::x11_window;
use crate::ui::tray::AppCommand;
use crossbeam_channel::Sender;
use glib;
use gtk4::prelude::*;
use gtk4::{
    gio, Application, ApplicationWindow, Box as GtkBox, DrawingArea, Label, LevelBar, MenuButton,
    Orientation, ScrolledWindow, TextView,
};
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

pub(crate) const CSS: &str = r#"
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

pub(crate) const PILL_W: i32 = 150;
pub(crate) const PILL_H: i32 = 40;
pub(crate) const WIN_W: i32 = 480;
pub(crate) const WIN_H: i32 = 260;
pub(crate) const WAVE_BARS: usize = 14;
pub(crate) const WAVE_W: i32 = 80;
pub(crate) const WAVE_H: i32 = 22;

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum PillPhase {
    Recording,
    RecordingPtt,
    Processing,
    Done,
}

#[derive(Clone, Copy)]
pub(crate) enum PillDot {
    Recording,
    Processing,
}

pub struct DictationPanel {
    window: ApplicationWindow,
    window_layout: GtkBox,
    status_label: Label,
    timer_label: Label,
    level_bar: LevelBar,
    text_view: TextView,
    toast_label: Label,
    pill_layout: GtkBox,
    pill_dot: Label,
    pill_waveform: DrawingArea,
    pill_phase: Rc<Cell<PillPhase>>,
    pill_timer: Label,
    level_meter: Rc<RefCell<Option<audio::LevelMeter>>>,
    level_history: Rc<RefCell<VecDeque<f32>>>,
    timer_source: Rc<RefCell<Option<glib::SourceId>>>,
    timer_seconds: Rc<RefCell<u32>>,
    base_text: Rc<RefCell<String>>,
    win_state: Rc<RefCell<WindowState>>,
    mode: Rc<Cell<PanelMode>>,
    mode_action: gio::SimpleAction,
    processing_started_at: Rc<Cell<Option<std::time::Instant>>>,
    prev_active_xid: Rc<RefCell<Option<String>>>,
    pending_auto_paste: Rc<Cell<bool>>,
}

impl DictationPanel {
    pub fn new(
        app: &Application,
        cmd_tx: Sender<AppCommand>,
        config: Rc<RefCell<Config>>,
    ) -> Self {
        install_css();
        let initial_mode = config.borrow().panel_mode;

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

        let outer = GtkBox::new(Orientation::Vertical, 0);
        outer.append(&window_layout);
        outer.append(&pill_layout);
        let window = build_window(app, initial_mode, &outer);
        set_initial_layout_visibility(initial_mode, &window_layout, &pill_layout);

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
            prev_active_xid: Rc::new(RefCell::new(None)),
            pending_auto_paste: Rc::new(Cell::new(false)),
        }
    }

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
            PillDot::Recording => {}
            PillDot::Processing => self.pill_dot.add_css_class("pill-dot-proc"),
        }
    }

    pub fn set_ptt_active(&self, active: bool) {
        if active {
            self.status_label.set_text("● Push-to-Talk");
            self.status_label.remove_css_class("status-rec");
            self.status_label.add_css_class("status-ptt");
            self.pill_dot.add_css_class("pill-dot-ptt");
            self.pill_phase.set(PillPhase::RecordingPtt);
        } else {
            self.status_label.set_text("● Aufnahme");
            self.status_label.remove_css_class("status-ptt");
            self.status_label.add_css_class("status-rec");
            self.pill_dot.remove_css_class("pill-dot-ptt");
            if self.pill_phase.get() == PillPhase::RecordingPtt {
                self.pill_phase.set(PillPhase::Recording);
            }
        }
        self.pill_waveform.queue_draw();
    }

    pub fn show_recording(&self, device: &cpal::Device) {
        self.stop_running_timer();
        self.prepare_text_buffer_for_new_recording();
        self.set_status_to_recording();
        self.reset_waveform_bars_to_zero();
        self.start_level_meter_for_device(device);
        self.start_recording_countdown_timer();
        self.present_panel_and_restore_focus();
    }

    fn stop_running_timer(&self) {
        if let Some(id) = self.timer_source.borrow_mut().take() {
            id.remove();
        }
    }

    fn prepare_text_buffer_for_new_recording(&self) {
        if self.mode.get() == PanelMode::Window && self.text_view.has_focus() {
            self.append_space_to_existing_text();
        } else {
            self.text_view.buffer().set_text("");
            *self.base_text.borrow_mut() = String::new();
        }
    }

    fn append_space_to_existing_text(&self) {
        let buf = self.text_view.buffer();
        let mut existing: String = buf.text(&buf.start_iter(), &buf.end_iter(), false).into();
        if !existing.is_empty() && !existing.ends_with(' ') {
            existing.push(' ');
            buf.insert(&mut buf.end_iter(), " ");
        }
        *self.base_text.borrow_mut() = existing;
    }

    fn set_status_to_recording(&self) {
        self.status_label.set_text("● Aufnahme");
        self.status_label.remove_css_class("status-proc");
        self.status_label.remove_css_class("status-idle");
        self.status_label.remove_css_class("status-ptt");
        self.status_label.add_css_class("status-rec");
        self.set_pill_dot_state(PillDot::Recording);
        self.pill_phase.set(PillPhase::Recording);
        self.pill_timer.set_text("00:00");
    }

    fn reset_waveform_bars_to_zero(&self) {
        for v in self.level_history.borrow_mut().iter_mut() {
            *v = 0.0;
        }
    }

    fn start_level_meter_for_device(&self, device: &cpal::Device) {
        *self.level_meter.borrow_mut() = None;
        match audio::LevelMeter::start(device) {
            Ok(meter) => {
                *self.level_meter.borrow_mut() = Some(meter);
                self.start_level_bar_update_timer();
                self.start_waveform_history_update_timer();
            }
            Err(e) => eprintln!("[panel] level meter: {e}"),
        }
    }

    fn start_level_bar_update_timer(&self) {
        let meter_rc = Rc::clone(&self.level_meter);
        let bar = self.level_bar.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
            match meter_rc.borrow().as_ref() {
                Some(m) => { bar.set_value(m.get() as f64); glib::ControlFlow::Continue }
                None => { bar.set_value(0.0); glib::ControlFlow::Break }
            }
        });
    }

    fn start_waveform_history_update_timer(&self) {
        let meter_rc = Rc::clone(&self.level_meter);
        let hist = Rc::clone(&self.level_history);
        let area = self.pill_waveform.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(70), move || {
            let Some(level) = meter_rc.borrow().as_ref().map(|m| m.get()) else {
                return glib::ControlFlow::Break;
            };
            let mut h = hist.borrow_mut();
            if h.len() == WAVE_BARS { h.pop_front(); }
            h.push_back(level);
            drop(h);
            area.queue_draw();
            glib::ControlFlow::Continue
        });
    }

    fn start_recording_countdown_timer(&self) {
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
    }

    fn present_panel_and_restore_focus(&self) {
        let prev_active = x11_window::active_window_id();
        *self.prev_active_xid.borrow_mut() = prev_active.clone();

        gtk4::prelude::GtkWindowExt::set_focus(&self.window, None::<&gtk4::Widget>);
        self.window.present();

        let win = self.window.clone();
        glib::timeout_add_local_once(std::time::Duration::from_millis(50), move || {
            position_next_to_cursor(&win);
            let our_xid = x11_window::window_xid(&win);
            if let Some(xid) = our_xid { x11_window::activate_window(xid); }
            if let Some(ref xid) = prev_active { x11_window::focus_window(xid); }
            if let Some(xid) = our_xid { x11_window::raise_window(xid); }
        });
    }

    pub fn show_processing(&self) {
        *self.level_meter.borrow_mut() = None;
        self.stop_running_timer();
        self.set_status_to_processing();
        self.processing_started_at.set(Some(std::time::Instant::now()));
        self.start_processing_animation();
    }

    fn set_status_to_processing(&self) {
        self.status_label.set_text("⏳ Verarbeitung…");
        self.status_label.remove_css_class("status-rec");
        self.status_label.remove_css_class("status-idle");
        self.status_label.remove_css_class("status-ptt");
        self.status_label.add_css_class("status-proc");
        self.set_pill_dot_state(PillDot::Processing);
        self.pill_phase.set(PillPhase::Processing);
    }

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
                    let phase_offset = i as f32 / count.max(1) as f32 * std::f32::consts::TAU;
                    let v = (t * 5.5 - phase_offset).sin() * 0.5 + 0.5;
                    let amp = 0.18 + 0.62 * v;
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
        let to_insert = space_join(&existing, seg);
        buf.insert(&mut buf.end_iter(), &to_insert);
    }

    pub fn finish(&self, full_text: &str, cfg: &Config, history: Rc<RefCell<History>>) {
        self.set_status_to_idle();
        let remaining = self.time_remaining_in_minimum_processing_window();

        if full_text.is_empty() {
            self.dismiss_pill_after_empty_result(remaining);
            return;
        }

        let auto_paste = self.pending_auto_paste.replace(false);
        self.show_completion_toast(auto_paste);
        self.copy_transcript_or_schedule_auto_paste(full_text, auto_paste, remaining);
        self.trigger_done_animation_in_icon_mode(remaining);
        save_transcription_to_history(full_text, cfg, history);
    }

    fn set_status_to_idle(&self) {
        self.status_label.set_text("○ Bereit");
        self.status_label.remove_css_class("status-proc");
        self.status_label.remove_css_class("status-rec");
        self.status_label.remove_css_class("status-ptt");
        self.status_label.add_css_class("status-idle");
        self.timer_label.set_text("");
    }

    fn time_remaining_in_minimum_processing_window(&self) -> std::time::Duration {
        let min_processing = std::time::Duration::from_millis(600);
        self.processing_started_at
            .get()
            .and_then(|t| min_processing.checked_sub(t.elapsed()))
            .unwrap_or(std::time::Duration::ZERO)
    }

    fn dismiss_pill_after_empty_result(&self, delay: std::time::Duration) {
        if self.mode.get() != PanelMode::Icon {
            return;
        }
        let panel_window = self.window.clone();
        let win_state = Rc::clone(&self.win_state);
        let phase = Rc::clone(&self.pill_phase);
        let dot = self.pill_dot.clone();
        glib::timeout_add_local_once(delay, move || {
            phase.set(PillPhase::Recording);
            dot.remove_css_class("pill-dot-proc");
            dot.remove_css_class("pill-dot-done");
            save_window_position(&panel_window, &win_state);
            panel_window.set_visible(false);
        });
    }

    fn show_completion_toast(&self, auto_paste: bool) {
        self.toast_label.set_text(if auto_paste { "✓ Eingefügt" } else { "✓ In Zwischenablage kopiert" });
        let lbl = self.toast_label.clone();
        glib::timeout_add_local_once(std::time::Duration::from_secs(3), move || lbl.set_text(""));
    }

    fn copy_transcript_or_schedule_auto_paste(
        &self,
        full_text: &str,
        auto_paste: bool,
        remaining: std::time::Duration,
    ) {
        let Some(display) = gtk4::gdk::Display::default() else { return };
        let clipboard = display.clipboard();
        if auto_paste {
            self.paste_transcript_and_restore_clipboard(full_text, clipboard, remaining);
        } else {
            clipboard.set_text(full_text);
        }
    }

    fn paste_transcript_and_restore_clipboard(
        &self,
        full_text: &str,
        clipboard: gtk4::gdk::Clipboard,
        remaining: std::time::Duration,
    ) {
        let target = self.prev_active_xid.borrow().clone();
        let full_owned = full_text.to_string();
        let cb_for_callback = clipboard.clone();
        clipboard.read_text_async(None::<&gio::Cancellable>, move |result| {
            let prev_clipboard_text: Option<String> = result.ok().flatten().map(|s| s.to_string());
            cb_for_callback.set_text(&full_owned);
            schedule_auto_paste(target, remaining);
            let cb_restore = cb_for_callback.clone();
            let restore_after = remaining + std::time::Duration::from_millis(800);
            glib::timeout_add_local_once(restore_after, move || {
                match prev_clipboard_text {
                    Some(t) => cb_restore.set_text(&t),
                    None => cb_restore.set_text(""),
                }
            });
        });
    }

    fn trigger_done_animation_in_icon_mode(&self, delay: std::time::Duration) {
        if self.mode.get() != PanelMode::Icon {
            return;
        }
        let dot = self.pill_dot.clone();
        let phase = Rc::clone(&self.pill_phase);
        let timer_lbl = self.pill_timer.clone();
        let hist = Rc::clone(&self.level_history);
        let area = self.pill_waveform.clone();
        let panel_window = self.window.clone();
        let win_state = Rc::clone(&self.win_state);
        glib::timeout_add_local_once(delay, move || {
            phase.set(PillPhase::Done);
            dot.remove_css_class("pill-dot-proc");
            dot.add_css_class("pill-dot-done");
            timer_lbl.set_text("");
            start_done_animation(hist, area, phase, dot, panel_window, win_state);
        });
    }

    pub fn arm_auto_paste(&self, on: bool) {
        self.pending_auto_paste.set(on);
    }

    pub fn present(&self) {
        self.window.present();
    }

    pub fn hide(&self) {
        save_window_position(&self.window, &self.win_state);
        self.window.set_visible(false);
    }
}

pub(crate) fn space_join(existing: &str, seg: &str) -> String {
    if !existing.is_empty() && !existing.ends_with(' ') && !seg.starts_with(' ') {
        format!(" {seg}")
    } else {
        seg.to_string()
    }
}
