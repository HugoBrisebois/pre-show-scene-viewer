use rfd::FileDialog;
use symphonia::core::sample;
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
use realfft::RealFftPlanner;
use num_complex::Complex;




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

pub fn process(f32_samples :&[f32], block_size: usize) -> Vec<f32> {
    
    let length = f32_samples;

    // make a planner
    let mut real_planner = RealFftPlanner::<f64>::new();

    // create an FFT
    let r2c = real_planner.plan_fft_forward()

    let mut indata = r2c.make_input_vec();

    // making a vector to store the spectrum
    let mut spectrum = r2c.make_output_vec();

    // check the size of the data input and output
    assert_eq!(indata.len, length);
    assert_eq!(spectrum.len, length/2+1);

    // forward transform the signal
    r2c.process(&mut indata, &mut spectrum).unwrap();

    // create a vector for storing the output
    let mut outdata = r2c.make_output_vec();
    assert_eq!(outdata.len, length);k
}