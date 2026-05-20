//! Thin wrapper around `xdotool` and the GDK monitor API.
//!
//! Centralises every shell-out to xdotool plus the conversion from GDK's
//! logical-pixel geometry to X11 physical-pixel coordinates that the rest
//! of the panel needs for absolute positioning.

use glib::object::Cast;
use gtk4::prelude::*;
use std::process::Command;

// ── xdotool I/O ────────────────────────────────────────────────────────────

pub fn cursor_position() -> Option<(i32, i32)> {
    let out = Command::new("xdotool")
        .args(["getmouselocation", "--shell"])
        .output()
        .ok()?;
    parse_xy_kv(&String::from_utf8_lossy(&out.stdout))
}

pub fn active_window_id() -> Option<String> {
    let out = Command::new("xdotool").arg("getactivewindow").output().ok()?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}

pub fn move_window(xid: u64, x: i32, y: i32) {
    let _ = Command::new("xdotool")
        .args(["windowmove", &xid.to_string(), &x.to_string(), &y.to_string()])
        .status();
}

pub fn activate_window(xid: u64) {
    let _ = Command::new("xdotool")
        .args(["windowactivate", "--sync", &xid.to_string()])
        .status();
}

/// Returns the X11 WM_CLASS for the given window as `"instance class"`
/// (both fields joined by a space). None on lookup failure. Uses `xprop`
/// rather than `xdotool getwindowclassname` because the latter is missing
/// in older xdotool versions.
pub fn window_class(xid: u64) -> Option<String> {
    let out = Command::new("xprop")
        .args(["-id", &xid.to_string(), "WM_CLASS"])
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout);
    parse_wm_class(&s)
}

fn parse_wm_class(xprop_line: &str) -> Option<String> {
    // expected: WM_CLASS(STRING) = "instance", "class"
    let rhs = xprop_line.split('=').nth(1)?;
    let parts: Vec<String> = rhs
        .split('"')
        .enumerate()
        .filter(|(i, _)| i % 2 == 1) // odd indices are inside quotes
        .map(|(_, s)| s.to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" "))
    }
}

pub fn focus_window(xid: &str) {
    let _ = Command::new("xdotool")
        .args(["windowfocus", "--sync", xid])
        .status();
}

pub fn raise_window(xid: u64) {
    let _ = Command::new("xdotool")
        .args(["windowraise", &xid.to_string()])
        .status();
}

/// Returns (x, y, width, height) in X11 physical pixels.
pub fn window_geometry(xid: u64) -> Option<(i32, i32, i32, i32)> {
    let out = Command::new("xdotool")
        .args(["getwindowgeometry", "--shell", &xid.to_string()])
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout);
    let mut x = None;
    let mut y = None;
    let mut w = None;
    let mut h = None;
    for line in s.lines() {
        if let Some(v) = line.strip_prefix("X=") { x = v.parse().ok(); }
        else if let Some(v) = line.strip_prefix("Y=") { y = v.parse().ok(); }
        else if let Some(v) = line.strip_prefix("WIDTH=") { w = v.parse().ok(); }
        else if let Some(v) = line.strip_prefix("HEIGHT=") { h = v.parse().ok(); }
    }
    match (x, y, w, h) {
        (Some(x), Some(y), Some(w), Some(h)) => Some((x, y, w, h)),
        _ => None,
    }
}

// ── GDK glue ───────────────────────────────────────────────────────────────

pub fn window_xid(window: &gtk4::ApplicationWindow) -> Option<u64> {
    window
        .surface()
        .and_then(|s| s.downcast::<gdk4_x11::X11Surface>().ok())
        .map(|x| x.xid())
}

/// GDK monitor whose physical-pixel rectangle contains (x, y).
pub fn monitor_containing(x: i32, y: i32) -> Option<gtk4::gdk::Monitor> {
    let display = gtk4::gdk::Display::default()?;
    let monitors = display.monitors();
    for i in 0..monitors.n_items() {
        let obj = monitors.item(i)?;
        if let Ok(mon) = obj.downcast::<gtk4::gdk::Monitor>() {
            let (px, py, pw, ph) = monitor_geometry_physical(&mon);
            if x >= px && x < px + pw && y >= py && y < py + ph {
                return Some(mon);
            }
        }
    }
    None
}

/// GDK reports monitor geometry in logical pixels; X11 windowmove takes
/// physical pixels. Scale once here so callers can stay in physical space.
pub fn monitor_geometry_physical(mon: &gtk4::gdk::Monitor) -> (i32, i32, i32, i32) {
    let geo = mon.geometry();
    let scale = mon.scale_factor().max(1);
    (
        geo.x() * scale,
        geo.y() * scale,
        geo.width() * scale,
        geo.height() * scale,
    )
}

// ── internal ───────────────────────────────────────────────────────────────

fn parse_xy_kv(text: &str) -> Option<(i32, i32)> {
    let mut x = None;
    let mut y = None;
    for line in text.lines() {
        if let Some(v) = line.strip_prefix("X=") { x = v.parse().ok(); }
        else if let Some(v) = line.strip_prefix("Y=") { y = v.parse().ok(); }
    }
    x.zip(y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_xdotool_shell_output() {
        let s = "X=1234\nY=567\nSCREEN=0\nWINDOW=0\n";
        assert_eq!(parse_xy_kv(s), Some((1234, 567)));
    }

    #[test]
    fn returns_none_for_partial_kv() {
        assert_eq!(parse_xy_kv("X=10\n"), None);
        assert_eq!(parse_xy_kv(""), None);
    }

    #[test]
    fn parses_wm_class_two_fields() {
        let s = "WM_CLASS(STRING) = \"ghostty\", \"com.mitchellh.ghostty\"\n";
        assert_eq!(parse_wm_class(s), Some("ghostty com.mitchellh.ghostty".into()));
    }

    #[test]
    fn parses_wm_class_one_field() {
        let s = "WM_CLASS(STRING) = \"Alacritty\"\n";
        assert_eq!(parse_wm_class(s), Some("Alacritty".into()));
    }

    #[test]
    fn parses_wm_class_missing() {
        assert_eq!(parse_wm_class("WM_CLASS:  not found.\n"), None);
        assert_eq!(parse_wm_class(""), None);
    }
}
