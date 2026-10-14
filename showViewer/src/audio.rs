use realfft::RealFftPlanner;
use rfd::FileDialog;
use std::fs::File;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CODEC_TYPE_NULL, DecoderOptions};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

pub struct AudioTrack {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub duration_seconds: f64,
}

pub fn open_file() -> Option<PathBuf> {
    FileDialog::new()
        .add_filter(
            "Audio",
            &["mp3", "wav", "flac", "ogg", "opus", "m4a", "aac"],
        )
        .pick_file()
}

pub fn analyze(path: &Path) -> Result<AudioTrack, Box<dyn std::error::Error>> {
    let file = File::open(path)?;
    let mms = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|extension| extension.to_str()) {
        hint.with_extension(extension);
    }

    let probed = symphonia::default::get_probe().format(
        &hint,
        mms,
        &FormatOptions::default(),
        &MetadataOptions::default(),
    )?;

    let mut format = probed.format;
    let (track_id, codec_params) = format
        .tracks()
        .iter()
        .find(|track| track.codec_params.codec != CODEC_TYPE_NULL)
        .map(|track| (track.id, track.codec_params.clone()))
        .ok_or_else(|| {
            std::io::Error::new(ErrorKind::InvalidData, "no supported audio track found")
        })?;
    let mut decoder =
        symphonia::default::get_codecs().make(&codec_params, &DecoderOptions::default())?;

    let mut samples = Vec::new();
    let mut sample_rate = codec_params.sample_rate.unwrap_or(0);
    let mut channel_count = codec_params
        .channels
        .map(|channels| channels.count())
        .unwrap_or(0);
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::IoError(error)) if error.kind() == ErrorKind::UnexpectedEof => {
                break;
            }
            Err(error) => return Err(Box::new(error)),
        };

        if packet.track_id() != track_id {
            continue;
        }

        match decoder.decode(&packet) {
            Ok(decoded) => {
                let spec = *decoded.spec();
                sample_rate = spec.rate;
                channel_count = spec.channels.count();
                let mut sample_buffer = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
                sample_buffer.copy_interleaved_ref(decoded);
                let channels = channel_count.max(1);
                samples.extend(
                    sample_buffer
                        .samples()
                        .chunks(channels)
                        .map(|frame| frame.iter().sum::<f32>() / frame.len() as f32),
                );
            }
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(error) => return Err(Box::new(error)),
        }
    }

    if sample_rate == 0 || channel_count == 0 || samples.is_empty() {
        return Err(std::io::Error::new(
            ErrorKind::InvalidData,
            "audio track contains no decodable samples",
        )
        .into());
    }
    let duration_seconds = samples.len() as f64 / f64::from(sample_rate);
    Ok(AudioTrack {
        samples,
        sample_rate,
        duration_seconds,
    })
}

pub fn process(
    f32_samples: &[f32],
    block_size: usize,
) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
    if block_size == 0 {
        return Err(std::io::Error::new(
            ErrorKind::InvalidInput,
            "block size must be greater than zero",
        )
        .into());
    }

    let mut real_planner = RealFftPlanner::<f32>::new();
    let r2c = real_planner.plan_fft_forward(block_size);
    let mut input = r2c.make_input_vec();
    let mut spectrum = r2c.make_output_vec();
    let mut magnitudes = Vec::with_capacity(
        f32_samples
            .len()
            .div_ceil(block_size)
            .saturating_mul(spectrum.len()),
    );

    for block in f32_samples.chunks(block_size) {
        input.fill(0.0);
        input[..block.len()].copy_from_slice(block);
        r2c.process(&mut input, &mut spectrum)
            .map_err(|error| std::io::Error::new(ErrorKind::InvalidInput, error.to_string()))?;
        magnitudes.extend(spectrum.iter().map(|bin| bin.norm()));
    }

    Ok(magnitudes)
}

#[cfg(test)]
mod tests {
    use super::analyze;
    use std::process::Command;

    #[test]
    fn decoded_audio_reports_sample_rate_and_full_duration() {
        if Command::new("ffmpeg").arg("-version").output().is_err() {
            eprintln!("Skipping audio metadata test: ffmpeg is not installed");
            return;
        }

        let path =
            std::env::temp_dir().join(format!("showviewer-audio-test-{}.wav", std::process::id()));
        let status = Command::new("ffmpeg")
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:sample_rate=44100",
                "-t",
                "0.25",
                "-ac",
                "2",
                "-c:a",
                "pcm_s16le",
            ])
            .arg(&path)
            .status()
            .expect("ffmpeg should create test audio");
        assert!(status.success());

        let track = analyze(&path).expect("the generated WAV should decode");
        assert_eq!(track.sample_rate, 44_100);
        assert!((track.duration_seconds - 0.25).abs() < 0.002);
        assert!((track.samples.len() as f64 - 11_025.0).abs() < 90.0);
        std::fs::remove_file(path).unwrap();
    }
}
