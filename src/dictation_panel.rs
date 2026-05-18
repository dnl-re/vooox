use crate::audio;
use crate::config::{Config, PanelMode};
use crate::history::{History, HistoryEntry};
use crate::tray::{TrayCommand, WHISPER_MODELS};
use crate::window_state::{monitor_key, WindowState};
use crossbeam_channel::Sender;
use std::collections::VecDeque;
use std::rc::Rc;
use std::cell::{Cell, RefCell};
use glib;
use gtk4::prelude::*;
use gtk4::{
    gio, Application, ApplicationWindow, Box as GtkBox, CssProvider, DrawingArea, Label, LevelBar,
    MenuButton, Orientation, ScrolledWindow, Separator, Stack, TextView,
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
.pill-check {
    color: #26a269;
    font-size: 16px;
    font-weight: bold;
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
    pill_stack: Stack,
    pill_waveform: DrawingArea,
    pill_phase: Rc<Cell<PillPhase>>,
    pill_timer: Label,
    // shared state
    level_meter: Rc<RefCell<Option<audio::LevelMeter>>>,
    level_history: Rc<RefCell<VecDeque<f32>>>,
    current_level: Rc<Cell<f32>>,
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
        cmd_tx: Sender<TrayCommand>,
        config: Rc<RefCell<Config>>,
    ) -> Self {
        let provider = CssProvider::new();
        provider.load_from_data(CSS);
        if let Some(display) = gtk4::gdk::Display::default() {
            gtk4::style_context_add_provider_for_display(
                &display,
                &provider,
                gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }

        let initial_mode = config.borrow().panel_mode;

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

        // ── kebab menu (header) ───────────────────────────────────────────
        let menu_model = gio::Menu::new();
        menu_model.append(Some("Verlauf"), Some("panel.history"));
        menu_model.append(Some("Einstellungen"), Some("panel.settings"));

        let model_section = gio::Menu::new();
        for m in WHISPER_MODELS {
            let item = gio::MenuItem::new(Some(m), None);
            item.set_action_and_target_value(
                Some("panel.model"),
                Some(&m.to_variant()),
            );
            model_section.append_item(&item);
        }
        menu_model.append_section(Some("Modell"), &model_section);

        let mode_section = gio::Menu::new();
        for (label, value) in [("Diktierfenster", "window"), ("Nur Icon", "icon")] {
            let item = gio::MenuItem::new(Some(label), None);
            item.set_action_and_target_value(
                Some("panel.mode"),
                Some(&value.to_variant()),
            );
            mode_section.append_item(&item);
        }
        menu_model.append_section(Some("Modus"), &mode_section);

        let section = gio::Menu::new();
        section.append(Some("Fenster schließen"), Some("panel.close"));
        section.append(Some("App beenden"), Some("panel.quit"));
        menu_model.append_section(None, &section);

        let menu_btn = MenuButton::builder()
            .icon_name("view-more-symbolic")
            .menu_model(&menu_model)
            .valign(gtk4::Align::Center)
            .build();
        menu_btn.add_css_class("flat");

        let header_box = GtkBox::new(Orientation::Horizontal, 8);
        header_box.set_margin_top(8);
        header_box.set_margin_bottom(8);
        header_box.set_margin_start(12);
        header_box.set_margin_end(12);
        header_box.append(&status_label);
        header_box.append(&timer_label);
        header_box.append(&level_bar);
        header_box.append(&menu_btn);

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

        // ── window-mode layout ────────────────────────────────────────────
        let window_layout = GtkBox::new(Orientation::Vertical, 0);
        window_layout.add_css_class("background");
        window_layout.add_css_class("panel-root");
        window_layout.append(&header_box);
        window_layout.append(&Separator::new(Orientation::Horizontal));
        window_layout.append(&text_scroll);
        window_layout.append(&toast_label);

        // ── icon-mode (pill) layout ───────────────────────────────────────
        let pill_layout = GtkBox::new(Orientation::Horizontal, 10);
        pill_layout.add_css_class("panel-pill");
        pill_layout.set_valign(gtk4::Align::Center);
        pill_layout.set_halign(gtk4::Align::Center);

        let pill_dot = Label::new(Some("●"));
        pill_dot.add_css_class("pill-dot");
        pill_dot.set_valign(gtk4::Align::Center);

        // waveform drawing area
        let level_history: Rc<RefCell<VecDeque<f32>>> = Rc::new(RefCell::new(
            std::iter::repeat(0.0).take(WAVE_BARS).collect(),
        ));
        let pill_phase: Rc<Cell<PillPhase>> = Rc::new(Cell::new(PillPhase::Recording));
        let waveform = DrawingArea::new();
        waveform.set_content_width(WAVE_W);
        waveform.set_content_height(WAVE_H);
        waveform.set_valign(gtk4::Align::Center);
        {
            let hist = Rc::clone(&level_history);
            let phase = Rc::clone(&pill_phase);
            waveform.set_draw_func(move |_, cr, w, h| {
                let hist = hist.borrow();
                let n = hist.len().max(1);
                let bar_w = (w as f64 / n as f64) * 0.55;
                let gap = (w as f64 / n as f64) - bar_w;
                let center_y = h as f64 / 2.0;
                match phase.get() {
                    PillPhase::Recording => cr.set_source_rgba(1.0, 0.32, 0.32, 0.95),
                    PillPhase::Processing => cr.set_source_rgba(1.0, 0.68, 0.18, 0.95),
                }
                for (i, &lvl) in hist.iter().enumerate() {
                    // Audio levels are logarithmic — sqrt() lifts quiet speech
                    // into a visible range while still letting peaks ride high.
                    let l = lvl.clamp(0.0, 1.0) as f64;
                    let scaled = (l.sqrt() * 2.2).min(1.0);
                    let bar_h = (scaled * h as f64 * 0.9).max(2.0);
                    let x = i as f64 * (bar_w + gap) + gap * 0.5;
                    let y = center_y - bar_h / 2.0;
                    // rounded rect via simple path
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
        }

        let pill_check = Label::new(Some("✓"));
        pill_check.add_css_class("pill-check");
        pill_check.set_valign(gtk4::Align::Center);

        let pill_stack = Stack::new();
        pill_stack.add_named(&waveform, Some("wave"));
        pill_stack.add_named(&pill_check, Some("done"));
        pill_stack.set_visible_child_name("wave");
        pill_stack.set_size_request(WAVE_W, WAVE_H);

        let pill_timer = Label::new(Some("00:00"));
        pill_timer.add_css_class("pill-timer");
        pill_timer.set_valign(gtk4::Align::Center);

        pill_layout.append(&pill_dot);
        pill_layout.append(&pill_stack);
        pill_layout.append(&pill_timer);

        // ── outer container (holds both layouts) ──────────────────────────
        let outer = GtkBox::new(Orientation::Vertical, 0);
        outer.append(&window_layout);
        outer.append(&pill_layout);

        let window = ApplicationWindow::builder()
            .application(app)
            .title("vooox")
            .default_width(if initial_mode == PanelMode::Icon { PILL_W } else { WIN_W })
            .default_height(if initial_mode == PanelMode::Icon { PILL_H } else { WIN_H })
            .decorated(false)
            .build();
        window.set_child(Some(&outer));

        // initial visibility based on saved mode
        match initial_mode {
            PanelMode::Window => {
                window_layout.set_visible(true);
                pill_layout.set_visible(false);
            }
            PanelMode::Icon => {
                window_layout.set_visible(false);
                pill_layout.set_visible(true);
            }
        }

        // ── action group ──────────────────────────────────────────────────
        let action_group = gio::SimpleActionGroup::new();
        for (name, cmd) in [
            ("history", TrayCommand::OpenHistory),
            ("settings", TrayCommand::OpenSettings),
            ("close", TrayCommand::HidePanel),
            ("quit", TrayCommand::Quit),
        ] {
            let action = gio::SimpleAction::new(name, None);
            let tx = cmd_tx.clone();
            let cmd_clone = cmd.clone();
            action.connect_activate(move |_, _| {
                let _ = tx.send(cmd_clone.clone());
            });
            action_group.add_action(&action);
        }

        // stateful radio action for model selection
        let current_model = config.borrow().model.clone();
        let model_action = gio::SimpleAction::new_stateful(
            "model",
            Some(glib::VariantTy::STRING),
            &current_model.to_variant(),
        );
        {
            let tx = cmd_tx.clone();
            model_action.connect_activate(move |action, param| {
                if let Some(p) = param {
                    if let Some(s) = p.get::<String>() {
                        action.set_state(&s.to_variant());
                        let _ = tx.send(TrayCommand::SetModel(s));
                    }
                }
            });
        }
        action_group.add_action(&model_action);

        // stateful radio action for panel mode
        let mode_action = gio::SimpleAction::new_stateful(
            "mode",
            Some(glib::VariantTy::STRING),
            &initial_mode.as_str().to_variant(),
        );
        {
            let tx = cmd_tx.clone();
            mode_action.connect_activate(move |action, param| {
                if let Some(p) = param {
                    if let Some(s) = p.get::<String>() {
                        if let Some(m) = PanelMode::from_str(&s) {
                            action.set_state(&s.to_variant());
                            let _ = tx.send(TrayCommand::SetPanelMode(m));
                        }
                    }
                }
            });
        }
        action_group.add_action(&mode_action);

        window.insert_action_group("panel", Some(&action_group));

        let win_state = Rc::new(RefCell::new(WindowState::load()));

        // hide instead of destroy when the user closes the window
        {
            let state = Rc::clone(&win_state);
            window.connect_close_request(move |win| {
                save_window_position(win, &state);
                win.hide();
                glib::Propagation::Stop
            });
        }

        // drag header to move the undecorated window (window mode)
        {
            let win = window.clone();
            let header = header_box.clone();
            let menu_btn_for_drag = menu_btn.clone();
            let drag = gtk4::GestureClick::new();
            drag.set_button(1);
            drag.connect_pressed(move |gesture, _n, x, y| {
                if let Some(picked) = header.pick(x, y, gtk4::PickFlags::DEFAULT) {
                    let mut w = Some(picked);
                    while let Some(cur) = w {
                        if cur.eq(&menu_btn_for_drag) {
                            return;
                        }
                        w = cur.parent();
                        if let Some(ref p) = w {
                            if p.eq(&header) {
                                break;
                            }
                        }
                    }
                }
                begin_move_from_gesture(&win, gesture, x, y);
            });
            header_box.add_controller(drag);
        }

        // drag pill body to move the undecorated window (icon mode)
        {
            let win = window.clone();
            let pill = pill_layout.clone();
            let drag = gtk4::GestureClick::new();
            drag.set_button(1);
            drag.connect_pressed(move |gesture, _n, x, y| {
                begin_move_from_gesture(&win, gesture, x, y);
            });
            pill.add_controller(drag);
        }

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
            pill_stack,
            pill_waveform: waveform,
            pill_phase,
            pill_timer,
            level_meter: Rc::new(RefCell::new(None)),
            level_history,
            current_level: Rc::new(Cell::new(0.0)),
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
                // ensure pill starts in waveform view
                self.pill_stack.set_visible_child_name("wave");
                self.set_pill_dot_state(PillDot::Recording);
                self.pill_phase.set(PillPhase::Recording);
            }
        }
    }

    fn set_pill_dot_state(&self, state: PillDot) {
        self.pill_dot.remove_css_class("pill-dot-proc");
        self.pill_dot.remove_css_class("pill-dot-done");
        match state {
            PillDot::Recording => { /* base .pill-dot class only — pulses red */ }
            PillDot::Processing => self.pill_dot.add_css_class("pill-dot-proc"),
            PillDot::Done => self.pill_dot.add_css_class("pill-dot-done"),
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
        self.status_label.add_css_class("status-rec");

        // icon-mode visuals
        self.pill_stack.set_visible_child_name("wave");
        self.set_pill_dot_state(PillDot::Recording);
        self.pill_phase.set(PillPhase::Recording);
        // reset waveform history
        {
            let mut hist = self.level_history.borrow_mut();
            for v in hist.iter_mut() {
                *v = 0.0;
            }
        }
        self.current_level.set(0.0);
        self.pill_timer.set_text("00:00");

        // start level meter — drives BOTH the LevelBar (window mode)
        // and the DrawingArea history (icon mode)
        *self.level_meter.borrow_mut() = None;
        match audio::LevelMeter::start(device) {
            Ok(meter) => {
                *self.level_meter.borrow_mut() = Some(meter);
                let meter_rc = Rc::clone(&self.level_meter);
                let bar = self.level_bar.clone();
                let level_cell = Rc::clone(&self.current_level);
                glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
                    if let Some(ref m) = *meter_rc.borrow() {
                        let v = m.get();
                        bar.set_value(v as f64);
                        level_cell.set(v);
                        glib::ControlFlow::Continue
                    } else {
                        bar.set_value(0.0);
                        level_cell.set(0.0);
                        glib::ControlFlow::Break
                    }
                });

                // waveform: shift level history every ~70 ms and redraw
                let level_cell2 = Rc::clone(&self.current_level);
                let hist = Rc::clone(&self.level_history);
                let area = self.pill_waveform.clone();
                let recording_flag = Rc::clone(&self.level_meter);
                glib::timeout_add_local(std::time::Duration::from_millis(70), move || {
                    if recording_flag.borrow().is_none() {
                        return glib::ControlFlow::Break;
                    }
                    {
                        let mut h = hist.borrow_mut();
                        if h.len() == WAVE_BARS {
                            h.pop_front();
                        }
                        h.push_back(level_cell2.get());
                    }
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

        // Capture currently active X11 window so we can restore focus after present.
        let prev_active: Option<String> = std::process::Command::new("xdotool")
            .arg("getactivewindow")
            .output()
            .ok()
            .and_then(|o| {
                let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                if s.is_empty() { None } else { Some(s) }
            });

        let was_hidden = !self.window.is_visible();
        gtk4::prelude::GtkWindowExt::set_focus(&self.window, None::<&gtk4::Widget>);
        self.window.present();

        let win = self.window.clone();
        let state = Rc::clone(&self.win_state);
        glib::timeout_add_local_once(std::time::Duration::from_millis(50), move || {
            if was_hidden {
                position_for_cursor(&win, &state);
            }
            let our_xid = window_xid(&win);
            if let Some(xid) = our_xid {
                let _ = std::process::Command::new("xdotool")
                    .args(["windowactivate", "--sync", &xid.to_string()])
                    .status();
            }
            if let Some(ref xid) = prev_active {
                let _ = std::process::Command::new("xdotool")
                    .args(["windowfocus", "--sync", xid])
                    .status();
            }
            if let Some(xid) = our_xid {
                let _ = std::process::Command::new("xdotool")
                    .args(["windowraise", &xid.to_string()])
                    .status();
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
        self.status_label.add_css_class("status-proc");

        // icon-mode: keep waveform visible but switch to traveling sine wave.
        self.set_pill_dot_state(PillDot::Processing);
        self.pill_phase.set(PillPhase::Processing);
        self.pill_stack.set_visible_child_name("wave");
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

        // icon-mode: hold processing animation for min duration, then swap
        // to the green check, then hide.
        if self.mode.get() == PanelMode::Icon {
            let stack = self.pill_stack.clone();
            let dot = self.pill_dot.clone();
            let phase = Rc::clone(&self.pill_phase);
            let timer_lbl = self.pill_timer.clone();
            let panel_window = self.window.clone();
            let win_state = Rc::clone(&self.win_state);
            glib::timeout_add_local_once(remaining, move || {
                // Stops the processing animation timer on next tick.
                phase.set(PillPhase::Recording);
                dot.remove_css_class("pill-dot-proc");
                dot.add_css_class("pill-dot-done");
                stack.set_visible_child_name("done");
                timer_lbl.set_text("");

                let panel_window2 = panel_window.clone();
                let win_state2 = win_state.clone();
                let dot2 = dot.clone();
                glib::timeout_add_local_once(std::time::Duration::from_millis(1600), move || {
                    save_window_position(&panel_window2, &win_state2);
                    panel_window2.hide();
                    dot2.remove_css_class("pill-dot-done");
                });
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

#[derive(Clone, Copy, PartialEq)]
enum PillPhase {
    Recording,
    Processing,
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

    let display = match gtk4::gdk::Display::default() {
        Some(d) => d,
        None => return,
    };
    let monitors = display.monitors();
    let mut target: Option<(i32, i32, i32, i32)> = None;
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

    let scale = window.scale_factor().max(1) as i32;
    let (default_w, default_h) = window.default_size();
    let logical_w = if window.width() > 10 { window.width() } else if default_w > 0 { default_w } else { 480 };
    let logical_h = if window.height() > 10 { window.height() } else if default_h > 0 { default_h } else { 520 };
    let win_w = logical_w * scale;
    let win_h = logical_h * scale;
    let margin = 40 * scale;

    let target_x = (mon_px + (mon_pw - win_w) / 2).max(mon_px);
    let target_y = (mon_py + mon_ph - win_h - margin).max(mon_py);

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

pub(crate) fn space_join(existing: &str, seg: &str) -> String {
    if !existing.is_empty() && !existing.ends_with(' ') && !seg.starts_with(' ') {
        format!(" {seg}")
    } else {
        seg.to_string()
    }
}
