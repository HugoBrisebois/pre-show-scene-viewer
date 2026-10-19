# ShowViewer

ShowViewer is a procedural visual and VJ loop generator with optional
audio-reactive control. Choose a scene and palette, then preview or export the
result. **VJ Loops** creates a randomized geometric composition; use **Generate
new VJ composition** to reshuffle it. Choose **Audio reactive** to let bass,
midrange, and treble shape the pattern, or **Self-running** to generate a
standalone animated loop without an audio source.

Choose **Multi-band VJ** for a different composition where each frequency band
has its own visual role: bass drives expanding concentric pulses, midrange bends
and rotates a six-sided shape, and treble adds radiant spokes and sparkling
points. It also supports audio-reactive and self-running playback.

When using audio-reactive mode, load a supported audio file and press **Play**
to run the selected visual with synchronized audio. Bass drives expanding
impacts and central flashes, midrange bends and twists the pattern, and treble
brings out fine sparkling points.

## Export

Choose an output resolution and export a full-length MP4 with the selected
audio, or select **VJ Loops** or **Multi-band VJ → Self-running** to export a
standalone 16-second visual loop without audio. Each VJ export gets a fresh
composition. Audio-backed exports include the selected track (H.264 video and
AAC audio at 30 fps); standalone loops contain H.264 video at 30 fps. Export
time depends on the duration, resolution, and hardware.

FFmpeg with the `libx264` encoder must be available on `PATH`. `ffplay` is
required for audio playback.
Reactive shader preview and rendering require an OpenGL 3.3-compatible renderer.
The audio decoder supports MP3, WAV, FLAC, Ogg, Opus, M4A, and AAC where the
selected Symphonia codecs support the file.

## Install

```sh
cargo install showViewer
```
