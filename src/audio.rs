use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleFormat, Stream, StreamConfig};
use std::sync::{Arc, Mutex};

pub struct DeviceInfo {
    pub name: String,    // raw ALSA name, stored in config
    pub display: String, // human-readable label for the UI
    pub device: Device,
}

/// Reads /proc/asound/cards and returns short_name → full_name mapping.
/// Example: "J380" → "Jabra Link 380"
fn alsa_card_names() -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    if let Ok(content) = std::fs::read_to_string("/proc/asound/cards") {
        for line in content.lines() {
            let line = line.trim();
            if let (Some(bs), Some(be)) = (line.find('['), line.find(']')) {
                let short = line[bs + 1..be].trim().to_string();
                if let Some(dash) = line.rfind(" - ") {
                    let full = line[dash + 3..].trim().to_string();
                    map.insert(short, full);
                }
            }
        }
    }
    map
}

fn make_display_name(name: &str, cards: &std::collections::HashMap<String, String>) -> String {
    match name {
        "default" => "Standard (ALSA)".to_string(),
        "pulse" => "PulseAudio / PipeWire (folgt Systemeinstellung)".to_string(),
        "pipewire" => "PipeWire".to_string(),
        _ => {
            if let Some(card_part) = name.split("CARD=").nth(1) {
                let short = card_part.split(',').next().unwrap_or(card_part);
                let prefix = name.split(':').next().unwrap_or("");
                let full = cards.get(short).map(|s| s.as_str()).unwrap_or(short);
                format!("{full} ({prefix})")
            } else {
                name.to_string()
            }
        }
    }
}

/// Keep only the minimal useful set: one entry per physical card plus pulse/pipewire.
/// This mirrors what GNOME Sound Settings shows (one device per card, via PipeWire).
fn is_useful_input_device(name: &str) -> bool {
    matches!(name, "pulse" | "pipewire" | "default")
        || name.starts_with("sysdefault:CARD=")
}

pub fn list_input_devices() -> Vec<DeviceInfo> {
    let host = cpal::default_host();
    let cards = alsa_card_names();
    let mut devices: Vec<DeviceInfo> = host
        .input_devices()
        .map(|iter| {
            iter.filter_map(|d| {
                let name = d.name().unwrap_or_else(|_| "Unknown".into());
                if !is_useful_input_device(&name) {
                    return None;
                }
                let display = make_display_name(&name, &cards);
                Some(DeviceInfo { name, display, device: d })
            })
            .collect()
        })
        .unwrap_or_default();

    // sort: pulse first (PipeWire system default), then "default", then rest
    devices.sort_by_key(|d| match d.name.as_str() {
        "pulse" => 0u8,
        "default" => 1,
        _ => 2,
    });
    devices
}

pub fn default_input_device() -> Option<Device> {
    cpal::default_host().default_input_device()
}

pub fn find_device_by_name(name: &str) -> Option<Device> {
    list_input_devices().into_iter().find(|d| d.name == name).map(|d| d.device)
}

// ── Recording ─────────────────────────────────────────────────────────────

pub struct Recorder {
    _stream: Stream,
    samples: Arc<Mutex<Vec<f32>>>,
    pub sample_rate: u32,
    pub channels: u16,
}

impl Recorder {
    pub fn start(device: &Device) -> Result<Self, Box<dyn std::error::Error>> {
        let supported = device.default_input_config()?;
        let sample_rate = supported.sample_rate(); // SampleRate = u32 in cpal 0.17
        let channels = supported.channels();
        let sample_format = supported.sample_format();
        // convert to StreamConfig before branching so we don't try to move in multiple arms
        let cfg: StreamConfig = supported.into();

        let samples: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));
        let samples_clone = Arc::clone(&samples);
        let err_fn = |e| eprintln!("[audio] stream error: {e}");

        let stream = match sample_format {
            SampleFormat::F32 => {
                device.build_input_stream(
                    &cfg,
                    move |data: &[f32], _| {
                        samples_clone.lock().unwrap().extend_from_slice(data);
                    },
                    err_fn,
                    None,
                )?
            }
            SampleFormat::I16 => {
                device.build_input_stream(
                    &cfg,
                    move |data: &[i16], _| {
                        let mut buf = samples_clone.lock().unwrap();
                        buf.extend(data.iter().map(|&s| s as f32 / i16::MAX as f32));
                    },
                    err_fn,
                    None,
                )?
            }
            SampleFormat::U16 => {
                device.build_input_stream(
                    &cfg,
                    move |data: &[u16], _| {
                        let mut buf = samples_clone.lock().unwrap();
                        buf.extend(
                            data.iter()
                                .map(|&s| (s as f32 / u16::MAX as f32) * 2.0 - 1.0),
                        );
                    },
                    err_fn,
                    None,
                )?
            }
            f => return Err(format!("unsupported sample format: {f:?}").into()),
        };
        stream.play()?;
        Ok(Recorder { _stream: stream, samples, sample_rate, channels })
    }

    /// Number of samples captured so far (without stopping the stream).
    pub fn sample_count(&self) -> usize {
        self.samples.lock().unwrap().len()
    }

    /// Clone all samples captured so far without stopping the stream.
    pub fn peek_samples(&self) -> (Vec<f32>, u32, u16) {
        let samples = self.samples.lock().unwrap().clone();
        (samples, self.sample_rate, self.channels)
    }

    pub fn stop_and_take(self) -> Vec<f32> {
        let Recorder { _stream, samples, .. } = self;
        drop(_stream); // stop stream → callback drops its Arc clone
        Arc::try_unwrap(samples)
            .map(|m| m.into_inner().unwrap())
            .unwrap_or_default()
    }
}

// ── Level meter ───────────────────────────────────────────────────────────

pub struct LevelMeter {
    _stream: Stream,
    pub level: Arc<Mutex<f32>>,
}

impl LevelMeter {
    pub fn start(device: &Device) -> Result<Self, Box<dyn std::error::Error>> {
        let supported = device.default_input_config()?;
        let sample_format = supported.sample_format();
        let cfg: StreamConfig = supported.into();
        let level: Arc<Mutex<f32>> = Arc::new(Mutex::new(0.0));
        let level_clone = Arc::clone(&level);
        let err_fn = |e| eprintln!("[level] stream error: {e}");

        let compute_rms = move |samples: &[f32]| {
            if samples.is_empty() {
                return;
            }
            let sum_sq: f32 = samples.iter().map(|s| s * s).sum();
            *level_clone.lock().unwrap() = (sum_sq / samples.len() as f32).sqrt();
        };

        let stream = match sample_format {
            SampleFormat::F32 => {
                device.build_input_stream(&cfg, move |d: &[f32], _| compute_rms(d), err_fn, None)?
            }
            SampleFormat::I16 => {
                device.build_input_stream(
                    &cfg,
                    move |d: &[i16], _| {
                        let v: Vec<f32> = d.iter().map(|&s| s as f32 / i16::MAX as f32).collect();
                        compute_rms(&v);
                    },
                    err_fn,
                    None,
                )?
            }
            _ => device.build_input_stream(&cfg, move |_: &[f32], _| {}, err_fn, None)?,
        };
        stream.play()?;
        Ok(LevelMeter { _stream: stream, level })
    }

    pub fn get(&self) -> f32 {
        *self.level.lock().unwrap()
    }
}

// ── WAV encoding ──────────────────────────────────────────────────────────

pub fn to_wav_bytes(samples: &[f32], sample_rate: u32, channels: u16) -> Vec<u8> {
    let spec = hound::WavSpec {
        channels,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut writer = hound::WavWriter::new(&mut buf, spec).expect("wav writer");
        for &s in samples {
            let i = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
            writer.write_sample(i).expect("wav sample");
        }
        writer.finalize().expect("wav finalize");
    }
    buf.into_inner()
}

pub fn to_mono(samples: &[f32], channels: u16) -> Vec<f32> {
    if channels == 1 {
        return samples.to_vec();
    }
    let ch = channels as usize;
    samples
        .chunks(ch)
        .map(|frame| frame.iter().sum::<f32>() / ch as f32)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_filter_keeps_only_useful_devices() {
        // kept: pulse, pipewire, default, sysdefault:CARD=*
        assert!(is_useful_input_device("pulse"));
        assert!(is_useful_input_device("pipewire"));
        assert!(is_useful_input_device("default"));
        assert!(is_useful_input_device("sysdefault:CARD=BRIO"));
        assert!(is_useful_input_device("sysdefault:CARD=J380"));

        // dropped: all ALSA aliases that duplicate sysdefault
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

    #[test]
    fn wav_roundtrip() {
        let sample_rate = 16_000u32;
        let samples: Vec<f32> = (0..sample_rate)
            .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / sample_rate as f32).sin())
            .collect();
        let wav = to_wav_bytes(&samples, sample_rate, 1);
        assert_eq!(&wav[0..4], b"RIFF");
        assert!(wav.len() > 44);
    }

    #[test]
    fn wav_header_fields() {
        let wav = to_wav_bytes(&vec![0.0f32; 100], 16000, 1);
        assert_eq!(&wav[8..12], b"WAVE");
    }

    #[test]
    fn to_mono_stereo() {
        let stereo = vec![0.5f32, -0.5, 0.3, -0.3];
        let mono = to_mono(&stereo, 2);
        assert_eq!(mono.len(), 2);
        assert!((mono[0] - 0.0).abs() < 1e-6);
    }

    #[test]
    fn to_mono_passthrough() {
        let samples = vec![0.1f32, 0.2, 0.3];
        assert_eq!(to_mono(&samples, 1), samples);
    }
}
