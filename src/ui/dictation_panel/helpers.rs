use super::{PillPhase, WIN_H, WIN_W};
use crate::storage::config::Config;
use crate::storage::history::{History, HistoryEntry};
use crate::storage::window_state::{monitor_key, WindowState};
use crate::system::x11_window;
use glib;
use gtk4::prelude::*;
use gtk4::{ApplicationWindow, DrawingArea, Label};
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

pub(super) fn position_next_to_cursor(window: &ApplicationWindow) {
    let Some((cx, cy)) = x11_window::cursor_position() else { return };
    let Some(mon) = x11_window::monitor_containing(cx, cy) else { return };
    let Some(xid) = x11_window::window_xid(window) else { return };

    let mon = x11_window::monitor_geometry_physical(&mon);
    let scale = window.scale_factor().max(1);
    let (default_w, default_h) = window.default_size();
    let logical_w = if window.width() > 10 { window.width() }
        else if default_w > 0 { default_w } else { WIN_W };
    let logical_h = if window.height() > 10 { window.height() }
        else if default_h > 0 { default_h } else { WIN_H };
    let win_w = logical_w * scale;
    let win_h = logical_h * scale;
    let margin = 16 * scale;

    let right_x = cx + margin;
    let x = if right_x + win_w <= mon.x + mon.width { right_x }
        else { (cx - margin - win_w).max(mon.x) };
    let y = (cy - win_h / 2).max(mon.y).min(mon.y + mon.height - win_h);

    x11_window::move_window(xid, x, y);
}

pub(super) fn save_window_position(window: &ApplicationWindow, state: &Rc<RefCell<WindowState>>) {
    let Some(xid) = x11_window::window_xid(window) else { return };
    let Some(geo) = x11_window::window_geometry(xid) else { return };
    let center = (geo.x + geo.width / 2, geo.y + geo.height / 2);
    let Some(mon) = x11_window::monitor_containing(center.0, center.1) else { return };
    let mut st = state.borrow_mut();
    st.set(monitor_key(&mon), (geo.x, geo.y));
    st.save();
}

pub(super) fn save_transcription_to_history(full_text: &str, cfg: &Config, history: Rc<RefCell<History>>) {
    let entry = HistoryEntry {
        text: full_text.to_string(),
        timestamp: crate::storage::history::now_rfc3339(),
        model: cfg.model.clone(),
        language: cfg.language.clone(),
    };
    history.borrow_mut().push(entry);
}

pub(super) fn begin_move_from_gesture(win: &ApplicationWindow, gesture: &gtk4::GestureClick, x: f64, y: f64) {
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

pub(super) fn start_done_animation(
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
            win.set_visible(false);
            dot.remove_css_class("pill-dot-done");
            phase.set(PillPhase::Recording);
            area.queue_draw();
            return glib::ControlFlow::Break;
        }

        let n = hist.borrow().len().max(1) as f32;
        let mut h = hist.borrow_mut();
        for (i, slot) in h.iter_mut().enumerate() {
            let bar_height = calculate_sweep_bar_height(i as f32, t, n, sweep_dur, fade_dur);
            *slot = bar_height * bar_height;
        }
        drop(h);
        area.queue_draw();
        glib::ControlFlow::Continue
    });
}

fn calculate_sweep_bar_height(bar_index: f32, t: f32, n: f32, sweep_dur: f32, fade_dur: f32) -> f32 {
    if t < sweep_dur {
        let progress = t / sweep_dur * n;
        let dist = (progress - bar_index).abs();
        let pulse = (1.0_f32 - dist / 1.4).max(0.0);
        let settled: f32 = if progress > bar_index + 0.4 { 0.45 } else { 0.0 };
        pulse.max(settled)
    } else {
        let fade_progress = ((t - sweep_dur) / fade_dur).clamp(0.0, 1.0);
        0.45 * (1.0 - fade_progress)
    }
}

pub(super) fn schedule_auto_paste(target_xid: Option<String>, after: std::time::Duration) {
    glib::timeout_add_local_once(after, move || {
        let key = match target_xid.as_deref().and_then(parse_xid) {
            Some(xid) => {
                x11_window::activate_window(xid);
                paste_key_for(xid)
            }
            None => "ctrl+v",
        };
        glib::timeout_add_local_once(std::time::Duration::from_millis(120), move || {
            if let Err(e) = std::process::Command::new("xdotool")
                .args(["key", key])
                .spawn()
            {
                eprintln!("[auto-paste] xdotool: {e} — install xdotool for auto-paste");
            }
        });
    });
}

fn parse_xid(s: &str) -> Option<u64> {
    s.parse::<u64>()
        .ok()
        .or_else(|| u64::from_str_radix(s.trim_start_matches("0x"), 16).ok())
}

fn paste_key_for(xid: u64) -> &'static str {
    match x11_window::window_class(xid) {
        Some(c) if is_terminal_class(&c) => "ctrl+shift+v",
        _ => "ctrl+v",
    }
}

fn is_terminal_class(class: &str) -> bool {
    let lc = class.to_lowercase();
    [
        "gnome-terminal", "konsole", "xterm", "alacritty", "kitty", "urxvt", "rxvt",
        "terminator", "tilix", "foot", "wezterm", "guake", "yakuake", "termite",
        "xfce4-terminal", "mate-terminal", "lxterminal", "deepin-terminal",
        "qterminal", "blackbox", "ptyxis", "ghostty",
    ]
    .iter()
    .any(|t| lc.contains(t))
}
