pub trait TextInjector: Send {
    fn type_text(&mut self, text: &str) -> Result<(), String>;
}

// ── helpers ───────────────────────────────────────────────────────────────

fn is_available(program: &str) -> bool {
    std::process::Command::new("which")
        .arg(program)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Copy `text` to the system clipboard.
/// Tries wl-copy (Wayland) then xclip (X11/XWayland).
fn copy_to_clipboard(text: &str) -> Result<(), String> {
    use std::io::Write;

    // wl-copy: native Wayland clipboard
    if is_available("wl-copy") {
        let mut child = std::process::Command::new("wl-copy")
            .stdin(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| format!("wl-copy spawn: {e}"))?;
        child.stdin.as_mut().unwrap().write_all(text.as_bytes())
            .map_err(|e| format!("wl-copy write: {e}"))?;
        child.wait().map_err(|e| format!("wl-copy wait: {e}"))?;
        return Ok(());
    }

    // xclip: X11 / XWayland (clipboard synced by compositor on GNOME)
    if is_available("xclip") {
        let mut child = std::process::Command::new("xclip")
            .args(["-selection", "clipboard"])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| format!("xclip spawn: {e}"))?;
        child.stdin.as_mut().unwrap().write_all(text.as_bytes())
            .map_err(|e| format!("xclip write: {e}"))?;
        child.wait().map_err(|e| format!("xclip wait: {e}"))?;
        return Ok(());
    }

    Err("no clipboard tool found (install wl-clipboard or xclip)".into())
}

/// Simulate Ctrl+V paste keystroke.
fn simulate_paste() -> Result<(), String> {
    // ydotool: works for Wayland-native and XWayland
    if is_available("ydotool") {
        let s = std::process::Command::new("ydotool")
            .args(["key", "29:1", "47:1", "47:0", "29:0"])
            .status()
            .map_err(|e| format!("ydotool key: {e}"))?;
        if s.success() { return Ok(()); }
    }

    // xdotool: works for XWayland apps
    if is_available("xdotool") {
        let s = std::process::Command::new("xdotool")
            .args(["key", "--clearmodifiers", "ctrl+v"])
            .status()
            .map_err(|e| format!("xdotool key: {e}"))?;
        if s.success() { return Ok(()); }
    }

    Err("no paste-key tool found (install xdotool or ydotool)".into())
}

// ── multi-strategy injector ───────────────────────────────────────────────
// Primary: clipboard + Ctrl+V paste (no dropped spaces, works for any length)
// Fallback: ydotool type → enigo (X11)

pub struct MultiInjector {
    enigo: Option<enigo::Enigo>,
}

impl MultiInjector {
    pub fn new() -> Self {
        use enigo::{Enigo, Settings};
        MultiInjector { enigo: Enigo::new(&Settings::default()).ok() }
    }
}

impl TextInjector for MultiInjector {
    fn type_text(&mut self, text: &str) -> Result<(), String> {
        // clipboard+paste: most reliable, no dropped spaces, any length
        if copy_to_clipboard(text).is_ok() {
            return simulate_paste();
        }

        // ydotool type: Wayland-native fallback
        if is_available("ydotool") {
            let s = std::process::Command::new("ydotool")
                .args(["type", "--", text])
                .status()
                .map_err(|e| format!("ydotool: {e}"))?;
            if s.success() { return Ok(()); }
        }

        // enigo: X11/XTest last resort
        if let Some(ref mut e) = self.enigo {
            use enigo::Keyboard;
            return e.text(text).map_err(|e| format!("enigo: {e}"));
        }

        Err(
            "No injection backend available.\n\
             Install: sudo apt install xclip xdotool"
                .into(),
        )
    }
}

// ── Wayland stub (kept for the WaylandInjectorStub name used in tests) ───

pub struct WaylandInjectorStub;

impl TextInjector for WaylandInjectorStub {
    fn type_text(&mut self, _text: &str) -> Result<(), String> {
        Err("Wayland text injection not yet supported".into())
    }
}

// ── factory ───────────────────────────────────────────────────────────────

pub fn create_injector() -> Result<Box<dyn TextInjector>, String> {
    Ok(Box::new(MultiInjector::new()))
}

// ── mock for tests ────────────────────────────────────────────────────────

pub struct MockInjector(pub Vec<String>);

impl TextInjector for MockInjector {
    fn type_text(&mut self, text: &str) -> Result<(), String> {
        self.0.push(text.to_string());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_collects_calls() {
        let mut inj = MockInjector(vec![]);
        inj.type_text("Hello").unwrap();
        inj.type_text(", world").unwrap();
        assert_eq!(inj.0, vec!["Hello", ", world"]);
    }

    #[test]
    fn wayland_stub_returns_err() {
        assert!(WaylandInjectorStub.type_text("x").is_err());
    }
}
