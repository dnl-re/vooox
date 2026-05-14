pub trait TextInjector: Send {
    fn type_text(&mut self, text: &str) -> Result<(), String>;
}

// ── subprocess helpers ────────────────────────────────────────────────────

fn cmd_type(program: &str, args: &[&str], text: &str) -> Result<(), String> {
    let mut full_args: Vec<&str> = args.to_vec();
    full_args.push(text);
    let status = std::process::Command::new(program)
        .args(&full_args)
        .status()
        .map_err(|e| format!("{program} not found: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{program} exited with {status}"))
    }
}

fn is_available(program: &str) -> bool {
    std::process::Command::new("which")
        .arg(program)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

// ── multi-strategy injector ───────────────────────────────────────────────
// Tries in order: ydotool (Wayland) → xdotool (X11/XWayland) → enigo (X11)

pub struct MultiInjector {
    enigo: Option<enigo::Enigo>,
}

impl MultiInjector {
    pub fn new() -> Self {
        use enigo::{Enigo, Settings};
        MultiInjector {
            enigo: Enigo::new(&Settings::default()).ok(),
        }
    }
}

impl TextInjector for MultiInjector {
    fn type_text(&mut self, text: &str) -> Result<(), String> {
        // ydotool: works for both Wayland-native and XWayland apps
        if is_available("ydotool") {
            if let Ok(()) = cmd_type("ydotool", &["type", "--"], text) {
                return Ok(());
            }
        }

        // xdotool: works for XWayland apps
        if is_available("xdotool") {
            if let Ok(()) = cmd_type("xdotool", &["type", "--clearmodifiers", "--"], text) {
                return Ok(());
            }
        }

        // enigo: X11/XTest fallback — silent no-op for Wayland-native windows
        if let Some(ref mut e) = self.enigo {
            use enigo::Keyboard;
            return e.text(text).map_err(|e| format!("enigo: {e}"));
        }

        Err(
            "No text injection backend available.\n\
             For Wayland: sudo apt install ydotool && systemctl --user enable --now ydotool\n\
             For X11/XWayland: sudo apt install xdotool"
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
