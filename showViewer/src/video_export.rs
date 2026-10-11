use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Sender, SyncSender};
use std::thread;

pub const EXPORT_FPS: u32 = 30;
pub const EXPORT_DURATION_SECONDS: u32 = 16;
pub const EXPORT_FRAME_COUNT: usize = (EXPORT_FPS * EXPORT_DURATION_SECONDS) as usize;

pub enum ExportMessage {
    Frame(Vec<u8>),
    Finish,
}

pub fn start_mp4_encoder(
    output: &Path,
    width: u32,
    height: u32,
    completion_sender: Sender<Result<(), String>>,
) -> SyncSender<ExportMessage> {
    let (frame_sender, frame_receiver) = mpsc::sync_channel(3);
    let output = output.to_owned();

    thread::spawn(move || {
        let result = encode_mp4(&output, width, height, frame_receiver);
        let _ = completion_sender.send(result);
    });

    frame_sender
}

fn encode_mp4(
    output: &Path,
    width: u32,
    height: u32,
    frame_receiver: mpsc::Receiver<ExportMessage>,
) -> Result<(), String> {
    let mut child = Command::new("ffmpeg")
        .args([
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
            "-an",
            "-c:v",
            "libx264",
            "-preset",
            "medium",
            "-crf",
            "18",
            "-pix_fmt",
            "yuv420p",
            "-movflags",
            "+faststart",
        ])
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
        let frame_sender = start_mp4_encoder(&output, 64, 64, completion_sender);
        for _ in 0..4 {
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
        std::fs::remove_file(output).unwrap();
    }
}
