use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{Device, SampleFormat, Stream, StreamConfig};
use std::sync::{Arc, Mutex};

pub struct CapturedAudio {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub channels: u16,
}

pub struct Recorder {
    _stream: Stream,
    samples: Arc<Mutex<Vec<f32>>>,
    pub sample_rate: u32,
    pub channels: u16,
}

impl Recorder {
    pub fn start(device: &Device) -> Result<Self, Box<dyn std::error::Error>> {
        let supported = device.default_input_config()?;
        let sample_rate = supported.sample_rate();
        let channels = supported.channels();
        let sample_format = supported.sample_format();
        let cfg: StreamConfig = supported.into();

        let buffer: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));
        let stream = build_recording_stream_for_format(device, &cfg, sample_format, Arc::clone(&buffer))?;
        stream.play()?;
        Ok(Recorder { _stream: stream, samples: buffer, sample_rate, channels })
    }

    /// Number of samples captured so far (without stopping the stream).
    pub fn sample_count(&self) -> usize {
        self.samples.lock().unwrap().len()
    }

    /// Clone all samples captured so far without stopping the stream.
    pub fn peek_samples(&self) -> CapturedAudio {
        let samples = self.samples.lock().unwrap().clone();
        CapturedAudio { samples, sample_rate: self.sample_rate, channels: self.channels }
    }

    pub fn stop_and_take(self) -> Vec<f32> {
        let Recorder { _stream, samples, .. } = self;
        drop(_stream); // stop stream → callback drops its Arc clone
        Arc::try_unwrap(samples)
            .map(|m| m.into_inner().unwrap())
            .unwrap_or_default()
    }
}

fn build_recording_stream_for_format(
    device: &Device,
    cfg: &StreamConfig,
    format: SampleFormat,
    buffer: Arc<Mutex<Vec<f32>>>,
) -> Result<Stream, Box<dyn std::error::Error>> {
    let err_fn = |e| eprintln!("[audio] stream error: {e}");
    let stream = match format {
        SampleFormat::F32 => record_f32_samples(device, cfg, buffer, err_fn)?,
        SampleFormat::I16 => record_i16_samples(device, cfg, buffer, err_fn)?,
        SampleFormat::U16 => record_u16_samples(device, cfg, buffer, err_fn)?,
        f => return Err(format!("unsupported sample format: {f:?}").into()),
    };
    Ok(stream)
}

fn record_f32_samples(
    device: &Device,
    cfg: &StreamConfig,
    buffer: Arc<Mutex<Vec<f32>>>,
    err_fn: impl Fn(cpal::StreamError) + Send + 'static,
) -> Result<Stream, cpal::BuildStreamError> {
    device.build_input_stream(
        cfg,
        move |data: &[f32], _| buffer.lock().unwrap().extend_from_slice(data),
        err_fn,
        None,
    )
}

fn record_i16_samples(
    device: &Device,
    cfg: &StreamConfig,
    buffer: Arc<Mutex<Vec<f32>>>,
    err_fn: impl Fn(cpal::StreamError) + Send + 'static,
) -> Result<Stream, cpal::BuildStreamError> {
    device.build_input_stream(
        cfg,
        move |data: &[i16], _| {
            buffer.lock().unwrap().extend(data.iter().map(|&s| s as f32 / i16::MAX as f32));
        },
        err_fn,
        None,
    )
}

fn record_u16_samples(
    device: &Device,
    cfg: &StreamConfig,
    buffer: Arc<Mutex<Vec<f32>>>,
    err_fn: impl Fn(cpal::StreamError) + Send + 'static,
) -> Result<Stream, cpal::BuildStreamError> {
    device.build_input_stream(
        cfg,
        move |data: &[u16], _| {
            buffer.lock().unwrap().extend(
                data.iter().map(|&s| (s as f32 / u16::MAX as f32) * 2.0 - 1.0),
            );
        },
        err_fn,
        None,
    )
}
