use crossbeam_channel::Sender;
use rdev::{Event, EventType, Key};
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq)]
pub struct Shortcut {
    pub modifiers: HashSet<Modifier>,
    pub key: Key,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Modifier {
    Ctrl,
    Shift,
    Alt,
    Super,
}

impl Shortcut {
    /// Parse a shortcut string like "ctrl+shift+space".
    pub fn parse(s: &str) -> Result<Self, String> {
        let parts: Vec<&str> = s.split('+').map(str::trim).collect();
        if parts.is_empty() {
            return Err("empty shortcut".into());
        }
        let mut mods = HashSet::new();
        let mut key_part = None;

        for part in &parts {
            match part.to_lowercase().as_str() {
                "ctrl" | "control" => { mods.insert(Modifier::Ctrl); }
                "shift" => { mods.insert(Modifier::Shift); }
                "alt" => { mods.insert(Modifier::Alt); }
                "super" | "meta" | "win" => { mods.insert(Modifier::Super); }
                other => {
                    if key_part.is_some() {
                        return Err(format!("multiple non-modifier keys: {other}"));
                    }
                    key_part = Some(*part);
                }
            }
        }

        let key_str = key_part.ok_or("no trigger key found")?;
        let key = parse_key(key_str)?;
        Ok(Shortcut { modifiers: mods, key })
    }
}

fn parse_key(s: &str) -> Result<Key, String> {
    match s.to_lowercase().as_str() {
        "space" => Ok(Key::Space),
        "return" | "enter" => Ok(Key::Return),
        "escape" | "esc" => Ok(Key::Escape),
        "tab" => Ok(Key::Tab),
        "backspace" => Ok(Key::Backspace),
        "f1" => Ok(Key::F1),
        "f2" => Ok(Key::F2),
        "f3" => Ok(Key::F3),
        "f4" => Ok(Key::F4),
        "f5" => Ok(Key::F5),
        "f6" => Ok(Key::F6),
        "f7" => Ok(Key::F7),
        "f8" => Ok(Key::F8),
        "f9" => Ok(Key::F9),
        "f10" => Ok(Key::F10),
        "f11" => Ok(Key::F11),
        "f12" => Ok(Key::F12),
        k if k.len() == 1 => {
            let c = k.chars().next().unwrap();
            match c {
                'a'..='z' => {
                    // rdev uses uppercase variants for letter keys
                    let upper = c.to_uppercase().next().unwrap();
                    key_from_char(upper).ok_or_else(|| format!("unknown key: {k}"))
                }
                _ => Err(format!("unsupported key character: {k}")),
            }
        }
        other => Err(format!("unknown key: {other}")),
    }
}

fn key_from_char(c: char) -> Option<Key> {
    match c {
        'A' => Some(Key::KeyA),
        'B' => Some(Key::KeyB),
        'C' => Some(Key::KeyC),
        'D' => Some(Key::KeyD),
        'E' => Some(Key::KeyE),
        'F' => Some(Key::KeyF),
        'G' => Some(Key::KeyG),
        'H' => Some(Key::KeyH),
        'I' => Some(Key::KeyI),
        'J' => Some(Key::KeyJ),
        'K' => Some(Key::KeyK),
        'L' => Some(Key::KeyL),
        'M' => Some(Key::KeyM),
        'N' => Some(Key::KeyN),
        'O' => Some(Key::KeyO),
        'P' => Some(Key::KeyP),
        'Q' => Some(Key::KeyQ),
        'R' => Some(Key::KeyR),
        'S' => Some(Key::KeyS),
        'T' => Some(Key::KeyT),
        'U' => Some(Key::KeyU),
        'V' => Some(Key::KeyV),
        'W' => Some(Key::KeyW),
        'X' => Some(Key::KeyX),
        'Y' => Some(Key::KeyY),
        'Z' => Some(Key::KeyZ),
        _ => None,
    }
}

// ── listener ─────────────────────────────────────────────────────────────

pub fn spawn_listener(shortcut: Shortcut, tx: Sender<()>) {
    std::thread::spawn(move || {
        let mut pressed: HashSet<Key> = HashSet::new();

        let callback = move |event: Event| {
            match event.event_type {
                EventType::KeyPress(k) => {
                    pressed.insert(k.clone());
                    if is_shortcut_active(&shortcut, &pressed, &k) {
                        let _ = tx.send(());
                    }
                }
                EventType::KeyRelease(k) => {
                    pressed.remove(&k);
                }
                _ => {}
            }
        };

        if let Err(e) = rdev::listen(callback) {
            eprintln!("[shortcuts] rdev error: {e:?}");
        }
    });
}

fn is_shortcut_active(shortcut: &Shortcut, pressed: &HashSet<Key>, trigger: &Key) -> bool {
    if trigger != &shortcut.key {
        return false;
    }
    let ctrl_ok = !shortcut.modifiers.contains(&Modifier::Ctrl)
        || pressed.contains(&Key::ControlLeft)
        || pressed.contains(&Key::ControlRight);
    let shift_ok = !shortcut.modifiers.contains(&Modifier::Shift)
        || pressed.contains(&Key::ShiftLeft)
        || pressed.contains(&Key::ShiftRight);
    let alt_ok = !shortcut.modifiers.contains(&Modifier::Alt)
        || pressed.contains(&Key::Alt)
        || pressed.contains(&Key::AltGr);
    let super_ok = !shortcut.modifiers.contains(&Modifier::Super)
        || pressed.contains(&Key::MetaLeft)
        || pressed.contains(&Key::MetaRight);
    ctrl_ok && shift_ok && alt_ok && super_ok
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ctrl_shift_space() {
        let s = Shortcut::parse("ctrl+shift+space").unwrap();
        assert!(s.modifiers.contains(&Modifier::Ctrl));
        assert!(s.modifiers.contains(&Modifier::Shift));
        assert_eq!(s.key, Key::Space);
    }

    #[test]
    fn parse_alt_f4() {
        let s = Shortcut::parse("alt+f4").unwrap();
        assert!(s.modifiers.contains(&Modifier::Alt));
        assert_eq!(s.key, Key::F4);
    }

    #[test]
    fn parse_single_key() {
        let s = Shortcut::parse("f9").unwrap();
        assert!(s.modifiers.is_empty());
        assert_eq!(s.key, Key::F9);
    }

    #[test]
    fn parse_letter_key() {
        let s = Shortcut::parse("ctrl+r").unwrap();
        assert!(s.modifiers.contains(&Modifier::Ctrl));
        assert_eq!(s.key, Key::KeyR);
    }

    #[test]
    fn parse_empty_fails() {
        assert!(Shortcut::parse("").is_err());
    }

    #[test]
    fn parse_unknown_key_fails() {
        assert!(Shortcut::parse("ctrl+foobar").is_err());
    }
}
