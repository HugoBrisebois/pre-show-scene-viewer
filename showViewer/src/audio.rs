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

pub fn open_file() -> Option<PathBuf> {
    FileDialog::new()
        .add_filter("audio", &["mp3", "wav", "flac"])
        .set_directory("/")
        .pick_file()
}

pub fn analyze(path: &Path) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
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
                let mut sample_buffer =
                    SampleBuffer::<f32>::new(decoded.capacity() as u64, *decoded.spec());
                sample_buffer.copy_interleaved_ref(decoded);
                samples.extend_from_slice(sample_buffer.samples());
            }
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(error) => return Err(Box::new(error)),
        }
    }

    Ok(samples)
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
