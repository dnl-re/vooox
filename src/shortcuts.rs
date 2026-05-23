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
        let parts = split_shortcut_parts(s)?;
        let modifiers = extract_modifiers(&parts);
        let key = extract_trigger_key(&parts)?;
        Ok(Shortcut { modifiers, key })
    }
}

fn split_shortcut_parts(s: &str) -> Result<Vec<String>, String> {
    let parts: Vec<String> = s.split('+').map(|p| p.trim().to_string()).collect();
    if parts.is_empty() || (parts.len() == 1 && parts[0].is_empty()) {
        return Err("empty shortcut".into());
    }
    Ok(parts)
}

fn extract_modifiers(parts: &[String]) -> HashSet<Modifier> {
    let mut mods = HashSet::new();
    for part in parts {
        if let Some(modifier) = parse_modifier(part) {
            mods.insert(modifier);
        }
    }
    mods
}

fn parse_modifier(s: &str) -> Option<Modifier> {
    match s.to_lowercase().as_str() {
        "ctrl" | "control" => Some(Modifier::Ctrl),
        "shift" => Some(Modifier::Shift),
        "alt" => Some(Modifier::Alt),
        "super" | "meta" | "win" => Some(Modifier::Super),
        _ => None,
    }
}

fn extract_trigger_key(parts: &[String]) -> Result<Key, String> {
    let non_modifier_parts: Vec<&str> = parts
        .iter()
        .filter(|p| parse_modifier(p).is_none())
        .map(|p| p.as_str())
        .collect();
    match non_modifier_parts.as_slice() {
        [] => Err("no trigger key found".into()),
        [single] => parse_key(single),
        [_, second, ..] => Err(format!("multiple non-modifier keys: {second}")),
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
        k if k.len() == 1 => parse_single_character_key(k),
        other => Err(format!("unknown key: {other}")),
    }
}

fn parse_single_character_key(k: &str) -> Result<Key, String> {
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

fn key_from_char(c: char) -> Option<Key> {
    match c {
        'A' => Some(Key::KeyA), 'B' => Some(Key::KeyB), 'C' => Some(Key::KeyC),
        'D' => Some(Key::KeyD), 'E' => Some(Key::KeyE), 'F' => Some(Key::KeyF),
        'G' => Some(Key::KeyG), 'H' => Some(Key::KeyH), 'I' => Some(Key::KeyI),
        'J' => Some(Key::KeyJ), 'K' => Some(Key::KeyK), 'L' => Some(Key::KeyL),
        'M' => Some(Key::KeyM), 'N' => Some(Key::KeyN), 'O' => Some(Key::KeyO),
        'P' => Some(Key::KeyP), 'Q' => Some(Key::KeyQ), 'R' => Some(Key::KeyR),
        'S' => Some(Key::KeyS), 'T' => Some(Key::KeyT), 'U' => Some(Key::KeyU),
        'V' => Some(Key::KeyV), 'W' => Some(Key::KeyW), 'X' => Some(Key::KeyX),
        'Y' => Some(Key::KeyY), 'Z' => Some(Key::KeyZ),
        _ => None,
    }
}

// ── listener ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShortcutEvent {
    Press,
    Release,
}

pub fn spawn_listener(shortcut: Shortcut, tx: Sender<ShortcutEvent>) {
    std::thread::spawn(move || {
        let mut pressed: HashSet<Key> = HashSet::new();
        let mut press_sent: bool = false;

        let callback = move |event: Event| {
            handle_key_event(event, &shortcut, &mut pressed, &mut press_sent, &tx);
        };

        if let Err(e) = rdev::listen(callback) {
            eprintln!("[shortcuts] rdev error: {e:?}");
        }
    });
}

fn handle_key_event(
    event: Event,
    shortcut: &Shortcut,
    pressed: &mut HashSet<Key>,
    press_sent: &mut bool,
    tx: &Sender<ShortcutEvent>,
) {
    match event.event_type {
        EventType::KeyPress(k) => {
            // rdev sends KeyPress repeatedly while the key is held —
            // only emit Press once per physical hold.
            let already_held = pressed.contains(&k);
            pressed.insert(k.clone());
            if !already_held && all_shortcut_keys_are_active(shortcut, pressed, &k) {
                *press_sent = true;
                let _ = tx.send(ShortcutEvent::Press);
            }
        }
        EventType::KeyRelease(k) => {
            pressed.remove(&k);
            if *press_sent && k == shortcut.key {
                *press_sent = false;
                let _ = tx.send(ShortcutEvent::Release);
            }
        }
        _ => {}
    }
}

fn all_shortcut_keys_are_active(shortcut: &Shortcut, pressed: &HashSet<Key>, trigger: &Key) -> bool {
    trigger == &shortcut.key
        && ctrl_state_matches(&shortcut.modifiers, pressed)
        && shift_state_matches(&shortcut.modifiers, pressed)
        && alt_state_matches(&shortcut.modifiers, pressed)
        && super_state_matches(&shortcut.modifiers, pressed)
}

fn ctrl_state_matches(required: &HashSet<Modifier>, pressed: &HashSet<Key>) -> bool {
    !required.contains(&Modifier::Ctrl)
        || pressed.contains(&Key::ControlLeft)
        || pressed.contains(&Key::ControlRight)
}

fn shift_state_matches(required: &HashSet<Modifier>, pressed: &HashSet<Key>) -> bool {
    !required.contains(&Modifier::Shift)
        || pressed.contains(&Key::ShiftLeft)
        || pressed.contains(&Key::ShiftRight)
}

fn alt_state_matches(required: &HashSet<Modifier>, pressed: &HashSet<Key>) -> bool {
    !required.contains(&Modifier::Alt)
        || pressed.contains(&Key::Alt)
        || pressed.contains(&Key::AltGr)
}

fn super_state_matches(required: &HashSet<Modifier>, pressed: &HashSet<Key>) -> bool {
    !required.contains(&Modifier::Super)
        || pressed.contains(&Key::MetaLeft)
        || pressed.contains(&Key::MetaRight)
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
