# ShowViewer

ShowViewer turns an audio track into audio-reactive visuals for preview and live
use. Load a supported audio file to analyze its spectrum, choose a scene and
palette, then use the transport below the render window to play, pause, or seek.
Playback and the visual playhead stay synchronized.

## Export

Choose an output resolution and export a full-length MP4. The video uses the
selected audio file's complete duration and includes its audio track (H.264 video
and AAC audio at 30 fps). Export time depends on the audio duration, resolution,
and hardware.

FFmpeg with the `libx264` encoder and `ffplay` must be available on `PATH`.
Reactive shader preview and rendering require an OpenGL 3.3-compatible renderer.
The audio decoder supports MP3, WAV, FLAC, Ogg, Opus, M4A, and AAC where the
selected Symphonia codecs support the file.

## Install

```sh
cargo install showViewer
```
