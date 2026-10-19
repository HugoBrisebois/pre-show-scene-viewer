use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Sender, SyncSender};
use std::thread;

pub const EXPORT_FPS: u32 = 30;

pub enum ExportMessage {
    Frame(Vec<u8>),
    Finish,
}

pub fn start_mp4_encoder(
    output: &Path,
    width: u32,
    height: u32,
    audio_path: Option<&Path>,
    duration_seconds: f64,
    completion_sender: Sender<Result<(), String>>,
) -> SyncSender<ExportMessage> {
    let (frame_sender, frame_receiver) = mpsc::sync_channel(3);
    let output = output.to_owned();
    let audio_path = audio_path.map(Path::to_owned);

    thread::spawn(move || {
        let result = encode_mp4(
            &output,
            width,
            height,
            audio_path.as_deref(),
            duration_seconds,
            frame_receiver,
        );
        let _ = completion_sender.send(result);
    });

    frame_sender
}

fn encode_mp4(
    output: &Path,
    width: u32,
    height: u32,
    audio_path: Option<&Path>,
    duration_seconds: f64,
    frame_receiver: mpsc::Receiver<ExportMessage>,
) -> Result<(), String> {
    let duration = format!("{duration_seconds:.6}");
    let mut command = Command::new("ffmpeg");
    command.args([
        "-hide_banner",
        "-loglevel",
        "error",
        "-y",
        "-f",
        "rawvideo",
        "-pixel_format",
        "rgba",
        "-video_size",
        &format!("{width}x{height}"),
        "-framerate",
        &EXPORT_FPS.to_string(),
        "-i",
        "pipe:0",
    ]);
    if let Some(audio_path) = audio_path {
        command.arg("-i").arg(audio_path);
    }
    command.args([
        "-map", "0:v:0", "-c:v", "libx264", "-preset", "medium", "-crf", "18", "-pix_fmt",
        "yuv420p", "-t", &duration,
    ]);
    if audio_path.is_some() {
        command.args(["-map", "1:a:0", "-c:a", "aac", "-b:a", "192k", "-shortest"]);
    } else {
        command.arg("-an");
    }
    let mut child = command
        .args(["-movflags", "+faststart"])
        .arg(output)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("Could not start FFmpeg: {error}"))?;

    let Some(mut stdin) = child.stdin.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err("FFmpeg did not provide an input stream".to_owned());
    };

    let write_result = loop {
        match frame_receiver.recv() {
            Ok(ExportMessage::Frame(frame)) => {
                if let Err(error) = stdin.write_all(&frame) {
                    break Err(format!("Could not send a frame to FFmpeg: {error}"));
                }
            }
            Ok(ExportMessage::Finish) | Err(_) => break Ok(()),
        }
    };

    drop(stdin);
    if let Err(error) = write_result {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error);
    }

    let output_status = child
        .wait_with_output()
        .map_err(|error| format!("Could not finish the FFmpeg export: {error}"))?;
    if !output_status.status.success() {
        return Err(format!(
            "FFmpeg failed: {}",
            String::from_utf8_lossy(&output_status.stderr).trim()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{ExportMessage, start_mp4_encoder};
    use std::process::Command;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn ffmpeg_encoder_writes_a_playable_mp4_when_x264_is_available() {
        let Ok(encoders) = Command::new("ffmpeg")
            .args(["-hide_banner", "-encoders"])
            .output()
        else {
            eprintln!("Skipping FFmpeg integration test: ffmpeg is not installed");
            return;
        };
        if !String::from_utf8_lossy(&encoders.stdout).contains("libx264") {
            eprintln!("Skipping FFmpeg integration test: libx264 is not available");
            return;
        }

        let output =
            std::env::temp_dir().join(format!("showviewer-export-test-{}.mp4", std::process::id()));
        let (completion_sender, completion_receiver) = mpsc::channel();
        let audio_path = std::env::temp_dir().join("showviewer-export-test.wav");
        let audio_status = Command::new("ffmpeg")
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "anullsrc=r=44100:cl=stereo",
                "-t",
                "0.2",
                "-c:a",
                "pcm_s16le",
            ])
            .arg(&audio_path)
            .status()
            .expect("ffmpeg should create test audio");
        assert!(audio_status.success());

        let frame_sender =
            start_mp4_encoder(&output, 64, 64, Some(&audio_path), 0.2, completion_sender);
        for _ in 0..6 {
            frame_sender
                .send(ExportMessage::Frame(vec![24; 64 * 64 * 4]))
                .expect("encoder thread should accept frames");
        }
        frame_sender
            .send(ExportMessage::Finish)
            .expect("encoder thread should accept finish");

        completion_receiver
            .recv_timeout(Duration::from_secs(30))
            .expect("encoder should finish")
            .expect("ffmpeg should encode the test frames");
        assert!(std::fs::metadata(&output).unwrap().len() > 0);
        if let Ok(probe) = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=codec_name,width,height",
                "-of",
                "csv=p=0",
            ])
            .arg(&output)
            .output()
        {
            assert_eq!(String::from_utf8_lossy(&probe.stdout).trim(), "h264,64,64");
        }
        let probe = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-show_entries",
                "stream=codec_name",
                "-of",
                "csv=p=0",
            ])
            .arg(&output)
            .output()
            .expect("ffprobe should inspect the exported MP4");
        let codecs = String::from_utf8_lossy(&probe.stdout);
        assert!(codecs.lines().any(|codec| codec == "h264"));
        assert!(codecs.lines().any(|codec| codec == "aac"));

        let duration = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-show_entries",
                "format=duration",
                "-of",
                "default=noprint_wrappers=1:nokey=1",
            ])
            .arg(&output)
            .output()
            .expect("ffprobe should report the exported duration");
        let duration = String::from_utf8_lossy(&duration.stdout)
            .trim()
            .parse::<f64>()
            .expect("ffprobe should report a numeric duration");
        assert!((0.19..=0.22).contains(&duration));
        std::fs::remove_file(output).unwrap();
        std::fs::remove_file(audio_path).unwrap();
    }

    #[test]
    fn ffmpeg_encoder_writes_a_silent_mp4_without_an_audio_source() {
        let Ok(encoders) = Command::new("ffmpeg")
            .args(["-hide_banner", "-encoders"])
            .output()
        else {
            eprintln!("Skipping FFmpeg integration test: ffmpeg is not installed");
            return;
        };
        if !String::from_utf8_lossy(&encoders.stdout).contains("libx264") {
            eprintln!("Skipping FFmpeg integration test: libx264 is not available");
            return;
        }

        let output =
            std::env::temp_dir().join(format!("showviewer-loop-test-{}.mp4", std::process::id()));
        let (completion_sender, completion_receiver) = mpsc::channel();
        let frame_sender = start_mp4_encoder(&output, 64, 64, None, 0.2, completion_sender);
        for _ in 0..6 {
            frame_sender
                .send(ExportMessage::Frame(vec![24; 64 * 64 * 4]))
                .expect("encoder thread should accept frames");
        }
        frame_sender
            .send(ExportMessage::Finish)
            .expect("encoder thread should accept finish");

        completion_receiver
            .recv_timeout(Duration::from_secs(30))
            .expect("encoder should finish")
            .expect("ffmpeg should encode the silent test frames");
        let probe = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-show_entries",
                "stream=codec_name",
                "-of",
                "csv=p=0",
            ])
            .arg(&output)
            .output()
            .expect("ffprobe should inspect the silent MP4");
        assert_eq!(String::from_utf8_lossy(&probe.stdout).trim(), "h264");
        std::fs::remove_file(output).unwrap();
    }
}
