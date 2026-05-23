use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{Device, SampleFormat, Stream, StreamConfig};
use std::sync::{Arc, Mutex};

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
        let stream = build_level_stream_for_format(device, &cfg, sample_format, Arc::clone(&level))?;
        stream.play()?;
        Ok(LevelMeter { _stream: stream, level })
    }

    pub fn get(&self) -> f32 {
        *self.level.lock().unwrap()
    }
}

fn build_level_stream_for_format(
    device: &Device,
    cfg: &StreamConfig,
    format: SampleFormat,
    level: Arc<Mutex<f32>>,
) -> Result<Stream, Box<dyn std::error::Error>> {
    let err_fn = |e| eprintln!("[level] stream error: {e}");
    let stream = match format {
        SampleFormat::F32 => {
            device.build_input_stream(cfg, move |d: &[f32], _| update_rms_level(d, &level), err_fn, None)?
        }
        SampleFormat::I16 => {
            device.build_input_stream(
                cfg,
                move |d: &[i16], _| {
                    let floats: Vec<f32> = d.iter().map(|&s| s as f32 / i16::MAX as f32).collect();
                    update_rms_level(&floats, &level);
                },
                err_fn,
                None,
            )?
        }
        _ => device.build_input_stream(cfg, move |_: &[f32], _| {}, err_fn, None)?,
    };
    Ok(stream)
}

fn update_rms_level(samples: &[f32], level: &Arc<Mutex<f32>>) {
    if samples.is_empty() {
        return;
    }
    let sum_of_squares: f32 = samples.iter().map(|s| s * s).sum();
    *level.lock().unwrap() = (sum_of_squares / samples.len() as f32).sqrt();
}

pub fn compute_rms_level(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_of_squares: f32 = samples.iter().map(|s| s * s).sum();
    (sum_of_squares / samples.len() as f32).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rms_level_of_silence_is_zero() {
        assert_eq!(compute_rms_level(&[]), 0.0);
        assert_eq!(compute_rms_level(&[0.0, 0.0, 0.0]), 0.0);
    }

    #[test]
    fn rms_level_of_full_scale_sine() {
        let samples: Vec<f32> = (0..1000)
            .map(|i| (2.0 * std::f32::consts::PI * i as f32 / 100.0).sin())
            .collect();
        let rms = compute_rms_level(&samples);
        assert!((rms - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.01);
    }
}
