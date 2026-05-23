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
