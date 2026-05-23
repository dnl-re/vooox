use cpal::traits::{DeviceTrait, HostTrait};
use cpal::Device;

pub struct DeviceInfo {
    pub name: String,    // raw ALSA name, stored in config
    pub display: String, // human-readable label for the UI
    pub device: Device,
}

pub fn list_input_devices() -> Vec<DeviceInfo> {
    let cards = alsa_card_names();
    let mut devices = collect_input_devices(&cards);
    sort_devices_by_preference(&mut devices);
    devices
}

fn collect_input_devices(cards: &std::collections::HashMap<String, String>) -> Vec<DeviceInfo> {
    let host = cpal::default_host();
    host.input_devices()
        .map(|iter| {
            iter.filter_map(|d| {
                let name = d.name().unwrap_or_else(|_| "Unknown".into());
                if should_skip_device(&name) {
                    return None;
                }
                let display = make_display_name(&name, cards);
                Some(DeviceInfo { name, display, device: d })
            })
            .collect()
        })
        .unwrap_or_default()
}

fn sort_devices_by_preference(devices: &mut Vec<DeviceInfo>) {
    devices.sort_by_key(|d| match d.name.as_str() {
        "pulse" => 0u8,
        "default" => 1,
        _ => 2,
    });
}

fn should_skip_device(name: &str) -> bool {
    !is_useful_input_device(name)
}

/// Keep only the minimal useful set: one entry per physical card plus pulse/pipewire.
/// This mirrors what GNOME Sound Settings shows (one device per card, via PipeWire).
fn is_useful_input_device(name: &str) -> bool {
    matches!(name, "pulse" | "pipewire" | "default")
        || name.starts_with("sysdefault:CARD=")
}

/// Reads /proc/asound/cards and returns short_name → full_name mapping.
/// Example: "J380" → "Jabra Link 380"
fn alsa_card_names() -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    let Ok(content) = std::fs::read_to_string("/proc/asound/cards") else {
        return map;
    };
    for line in content.lines() {
        if let Some((short, full)) = parse_alsa_card_line(line.trim()) {
            map.insert(short, full);
        }
    }
    map
}

fn parse_alsa_card_line(line: &str) -> Option<(String, String)> {
    let bracket_start = line.find('[')?;
    let bracket_end = line.find(']')?;
    let short = line[bracket_start + 1..bracket_end].trim().to_string();
    let dash_pos = line.rfind(" - ")?;
    let full = line[dash_pos + 3..].trim().to_string();
    Some((short, full))
}

fn make_display_name(name: &str, cards: &std::collections::HashMap<String, String>) -> String {
    match name {
        "default" => "Standard (ALSA)".to_string(),
        "pulse" => "PulseAudio / PipeWire (folgt Systemeinstellung)".to_string(),
        "pipewire" => "PipeWire".to_string(),
        _ => make_display_name_for_alsa_device(name, cards),
    }
}

fn make_display_name_for_alsa_device(name: &str, cards: &std::collections::HashMap<String, String>) -> String {
    let Some(card_part) = name.split("CARD=").nth(1) else {
        return name.to_string();
    };
    let short_id = card_part.split(',').next().unwrap_or(card_part);
    let device_type = name.split(':').next().unwrap_or("");
    let full_name = cards.get(short_id).map(|s| s.as_str()).unwrap_or(short_id);
    format!("{full_name} ({device_type})")
}

pub fn default_input_device() -> Option<Device> {
    cpal::default_host().default_input_device()
}

pub fn find_device_by_name(name: &str) -> Option<Device> {
    list_input_devices().into_iter().find(|d| d.name == name).map(|d| d.device)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_filter_keeps_only_useful_devices() {
        assert!(is_useful_input_device("pulse"));
        assert!(is_useful_input_device("pipewire"));
        assert!(is_useful_input_device("default"));
        assert!(is_useful_input_device("sysdefault:CARD=BRIO"));
        assert!(is_useful_input_device("sysdefault:CARD=J380"));

        assert!(!is_useful_input_device("null"));
        assert!(!is_useful_input_device("lavrate"));
        assert!(!is_useful_input_device("jack"));
        assert!(!is_useful_input_device("oss"));
        assert!(!is_useful_input_device("hw:CARD=BRIO,DEV=0"));
        assert!(!is_useful_input_device("plughw:CARD=BRIO,DEV=0"));
        assert!(!is_useful_input_device("dsnoop:CARD=BRIO,DEV=0"));
        assert!(!is_useful_input_device("front:CARD=BRIO,DEV=0"));
        assert!(!is_useful_input_device("usbstream:CARD=NVidia"));
    }
}
