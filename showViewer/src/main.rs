pub mod audio;
pub mod video_export;
pub mod visuals;
use eframe::egui;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const FFT_BLOCK_SIZE: usize = 1024;

fn main() -> eframe::Result<()> {
    // init the window
    let native_options = eframe::NativeOptions {
        renderer: eframe::Renderer::Glow,
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([960.0, 640.0]),
        ..Default::default()
    };
    eframe::run_native(
        "ShowViewer — Audio Reactive Visuals",
        native_options,
        Box::new(|cc| Ok(Box::new(ShowViewer::new(cc)))),
    )
}

struct ShowViewer {
    selected_file: Option<PathBuf>,
    audio_duration_seconds: f64,
    audio_sample_rate: u32,
    spectrum: Vec<f32>,
    error: Option<String>,
    worker_tx: Sender<WorkerMessage>,
    worker_rx: Receiver<WorkerMessage>,
    busy: Option<WorkerKind>,
    scene: visuals::Scene,
    vj_seed: f32,
    vj_audio_reactive: bool,
    preview_started: Instant,
    reactive_rendering: bool,
    shader_pipeline: Option<visuals::VjShaderPipeline>,
    shader_error: Option<String>,
    shader_warning: Option<String>,
    audio_smoother: visuals::AudioSmoother,
    primary_color: egui::Color32,
    secondary_color: egui::Color32,
    palette: Palette,
    export_resolution: ExportResolution,
    export_controller: Arc<Mutex<visuals::ExportController>>,
    exporting: bool,
    export_error: Option<String>,
    export_success: Option<String>,
    export_output: Option<PathBuf>,
    playback_process: Option<Child>,
    playback_started: Option<Instant>,
    playback_position_seconds: f64,
}

enum WorkerMessage {
    Decoded(Result<audio::AudioTrack, String>),
    Processed(Result<Vec<f32>, String>),
    Exported(Result<(), String>),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Palette {
    SageSand,
    IndigoRose,
    TealIvory,
    VioletAqua,
    Custom,
}

impl Palette {
    const PRESETS: [Self; 4] = [
        Self::SageSand,
        Self::IndigoRose,
        Self::TealIvory,
        Self::VioletAqua,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::SageSand => "Sage & Sand",
            Self::IndigoRose => "Indigo & Rose",
            Self::TealIvory => "Deep Teal & Ivory",
            Self::VioletAqua => "Violet & Aqua",
            Self::Custom => "Custom",
        }
    }

    fn colors(self) -> Option<(egui::Color32, egui::Color32)> {
        match self {
            Self::SageSand => Some((
                egui::Color32::from_rgb(112, 174, 158),
                egui::Color32::from_rgb(214, 183, 146),
            )),
            Self::IndigoRose => Some((
                egui::Color32::from_rgb(125, 136, 190),
                egui::Color32::from_rgb(207, 150, 166),
            )),
            Self::TealIvory => Some((
                egui::Color32::from_rgb(83, 157, 151),
                egui::Color32::from_rgb(226, 218, 190),
            )),
            Self::VioletAqua => Some((
                egui::Color32::from_rgb(157, 125, 186),
                egui::Color32::from_rgb(116, 194, 190),
            )),
            Self::Custom => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ExportResolution {
    Hd,
    FullHd,
}

impl ExportResolution {
    const ALL: [Self; 2] = [Self::Hd, Self::FullHd];

    fn dimensions(self) -> (u32, u32) {
        match self {
            Self::Hd => (1280, 720),
            Self::FullHd => (1920, 1080),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Hd => "HD 1280 x 720",
            Self::FullHd => "Full HD 1920 x 1080",
        }
    }
}

#[derive(Clone, Copy)]
enum WorkerKind {
    Decoding,
    Processing,
}

impl ShowViewer {
    fn select_scene(&mut self, scene: visuals::Scene) {
        self.scene = scene;
        self.reactive_rendering = scene.uses_shader();
        if scene.is_vj_mode() {
            self.vj_seed = visuals::new_vj_seed();
            self.preview_started = Instant::now();
            self.audio_smoother = visuals::AudioSmoother::default();
        }
    }

    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut app_visuals = egui::Visuals::dark();
        app_visuals.panel_fill = egui::Color32::from_rgb(13, 16, 22);
        app_visuals.window_fill = egui::Color32::from_rgb(24, 29, 38);
        app_visuals.extreme_bg_color = egui::Color32::from_rgb(10, 13, 19);
        app_visuals.selection.bg_fill = egui::Color32::from_rgb(46, 132, 117);
        cc.egui_ctx.set_visuals(app_visuals);

        let (worker_tx, worker_rx) = mpsc::channel();
        let (shader_pipeline, shader_error, shader_warning) = match cc.gl.as_ref() {
            Some(gl) => match visuals::VjShaderPipeline::new(Arc::clone(gl)) {
                Ok(pipeline) => {
                    let warning = pipeline.uniform_warning().map(str::to_owned);
                    (Some(pipeline), None, warning)
                }
                Err(error) => (None, Some(error), None),
            },
            None => (
                None,
                Some("Reactive shader requires the OpenGL renderer".to_owned()),
                None,
            ),
        };
        Self {
            selected_file: None,
            audio_duration_seconds: 0.0,
            audio_sample_rate: 0,
            spectrum: Vec::new(),
            error: None,
            worker_tx,
            worker_rx,
            busy: None,
            scene: visuals::Scene::Reactive,
            vj_seed: visuals::new_vj_seed(),
            vj_audio_reactive: true,
            preview_started: Instant::now(),
            reactive_rendering: false,
            shader_pipeline,
            shader_error,
            shader_warning,
            audio_smoother: visuals::AudioSmoother::default(),
            primary_color: egui::Color32::from_rgb(112, 174, 158),
            secondary_color: egui::Color32::from_rgb(214, 183, 146),
            palette: Palette::SageSand,
            export_resolution: ExportResolution::Hd,
            export_controller: Arc::new(Mutex::new(visuals::ExportController::default())),
            exporting: false,
            export_error: None,
            export_success: None,
            export_output: None,
            playback_process: None,
            playback_started: None,
            playback_position_seconds: 0.0,
        }
    }

    fn start_export(&mut self, output: PathBuf) {
        if let Err(error) = self.pause_audio() {
            self.export_error = Some(error);
            return;
        }
        let standalone_loop = self.scene.is_vj_mode() && !self.vj_audio_reactive;
        let audio_path = if standalone_loop {
            None
        } else {
            let Some(audio_path) = self.selected_file.as_deref() else {
                self.export_error = Some("Choose an audio file before exporting".to_owned());
                return;
            };
            if self.audio_duration_seconds <= 0.0 || !self.audio_duration_seconds.is_finite() {
                self.export_error =
                    Some("The selected audio file has no valid duration".to_owned());
                return;
            }
            Some(audio_path)
        };
        let duration_seconds = if standalone_loop {
            f64::from(visuals::LOOP_SECONDS)
        } else {
            self.audio_duration_seconds
        };
        let output = output.with_extension("mp4");
        self.export_output = Some(output.clone());
        let vj_loops = self.scene.is_vj_mode();
        let multi_band_vj = self.scene == visuals::Scene::MultiBandVj;
        let audio_reactive = !standalone_loop;
        if vj_loops {
            self.vj_seed = visuals::new_vj_seed();
        }
        let (width, height) = self.export_resolution.dimensions();
        let frame_count = (duration_seconds * f64::from(video_export::EXPORT_FPS)).ceil() as usize;
        let (completion_sender, completion_receiver) = mpsc::channel();
        let frame_sender = video_export::start_mp4_encoder(
            &output,
            width,
            height,
            audio_path,
            duration_seconds,
            completion_sender,
        );
        let worker_sender = self.worker_tx.clone();
        std::thread::spawn(move || {
            let result = completion_receiver
                .recv()
                .unwrap_or_else(|_| Err("MP4 encoder exited without a result".to_owned()));
            let _ = worker_sender.send(WorkerMessage::Exported(result));
        });

        let audio_controls = if audio_reactive {
            visuals::export_audio_controls(&self.spectrum, frame_count, video_export::EXPORT_FPS)
        } else {
            vec![[0.0; 4]; frame_count]
        };
        let capture = visuals::ExportCapture {
            width,
            height,
            frame_count,
            next_frame: 0,
            vj_loops,
            multi_band_vj,
            audio_reactive,
            vj_seed: self.vj_seed,
            audio_controls,
            primary_color: self.primary_color,
            secondary_color: self.secondary_color,
            frame_sender,
        };

        self.export_success = None;
        match self.export_controller.lock() {
            Ok(mut controller) => {
                controller.capture = Some(capture);
                controller.error = None;
                self.export_error = None;
                self.export_success = None;
                self.exporting = true;
                if !vj_loops {
                    self.scene = visuals::Scene::Reactive;
                }
                self.reactive_rendering = true;
            }
            Err(_) => {
                self.export_error =
                    Some("Could not start export: render queue is unavailable".into());
            }
        }
    }

    fn export_disabled_reason(&self) -> Option<&'static str> {
        if self.exporting {
            return Some("An export is already in progress.");
        }
        let standalone_loop = self.scene.is_vj_mode() && !self.vj_audio_reactive;
        if standalone_loop {
            if self.shader_pipeline.is_none() {
                return Some("VJ loop rendering requires an available OpenGL renderer.");
            }
            return None;
        }
        match self.busy {
            Some(WorkerKind::Decoding) => return Some("Wait for the audio file to load."),
            Some(WorkerKind::Processing) => {
                return Some("Wait for audio analysis to finish before exporting.");
            }
            None => {}
        }
        if self.selected_file.is_none() {
            return Some("Load an audio file to export a full-length render.");
        }
        if !self.audio_duration_seconds.is_finite() || self.audio_duration_seconds <= 0.0 {
            return Some("The selected audio file has no valid duration.");
        }
        if self.spectrum.is_empty() {
            return Some("Audio analysis is unavailable; try loading the audio file again.");
        }
        if self.shader_pipeline.is_none() {
            return Some("Reactive rendering requires an available OpenGL renderer.");
        }
        None
    }

    fn play_audio(&mut self) -> Result<(), String> {
        let Some(file) = self.selected_file.as_ref() else {
            return Err("Choose an audio file before starting playback".to_owned());
        };
        if self.audio_duration_seconds <= 0.0 {
            return Err("The selected audio file has no playable duration".to_owned());
        }
        if self.playback_position_seconds >= self.audio_duration_seconds {
            self.playback_position_seconds = 0.0;
        }

        let child = Command::new("ffplay")
            .args([
                "-nodisp",
                "-autoexit",
                "-loglevel",
                "error",
                "-ss",
                &format!("{:.3}", self.playback_position_seconds),
                "-i",
            ])
            .arg(file)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("Could not start audio playback with FFplay: {error}"))?;

        self.playback_process = Some(child);
        self.playback_started = Some(Instant::now());
        self.reactive_rendering = self.scene.uses_shader();
        Ok(())
    }

    fn pause_audio(&mut self) -> Result<(), String> {
        if let Some(started) = self.playback_started.take() {
            self.playback_position_seconds = (self.playback_position_seconds
                + started.elapsed().as_secs_f64())
            .min(self.audio_duration_seconds);
        }
        if let Some(mut child) = self.playback_process.take() {
            let already_finished = match child.try_wait() {
                Ok(status) => status.is_some(),
                Err(error) => {
                    self.playback_process = Some(child);
                    return Err(format!("Could not check audio playback: {error}"));
                }
            };
            if !already_finished && let Err(error) = child.kill() {
                self.playback_process = Some(child);
                return Err(format!("Could not stop audio playback: {error}"));
            }
            child
                .wait()
                .map_err(|error| format!("Could not stop audio playback: {error}"))?;
        }
        Ok(())
    }

    fn audio_position_seconds(&mut self) -> f64 {
        let Some(started) = self.playback_started else {
            return self.playback_position_seconds;
        };
        let position = (self.playback_position_seconds + started.elapsed().as_secs_f64())
            .min(self.audio_duration_seconds);
        let process_status = self.playback_process.as_mut().map(Child::try_wait);
        let process_finished = match process_status {
            Some(Ok(Some(status))) => {
                if !status.success() {
                    self.error = Some(format!("Audio playback stopped with exit status {status}"));
                }
                true
            }
            Some(Err(error)) => {
                self.error = Some(format!("Could not check audio playback: {error}"));
                true
            }
            _ => false,
        };
        let finished = position >= self.audio_duration_seconds || process_finished;
        if finished {
            self.playback_position_seconds = position;
            self.playback_started = None;
            if let Some(mut child) = self.playback_process.take()
                && let Err(error) = child.wait()
            {
                self.error = Some(format!("Could not finish audio playback: {error}"));
            }
        }
        position
    }
}

impl Drop for ShowViewer {
    fn drop(&mut self) {
        let _ = self.pause_audio();
    }
}

impl eframe::App for ShowViewer {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        while let Ok(message) = self.worker_rx.try_recv() {
            match message {
                WorkerMessage::Decoded(Ok(track)) => {
                    self.audio_duration_seconds = track.duration_seconds;
                    self.audio_sample_rate = track.sample_rate;
                    self.busy = Some(WorkerKind::Processing);
                    let sender = self.worker_tx.clone();
                    std::thread::spawn(move || {
                        let result = audio::process(&track.samples, FFT_BLOCK_SIZE)
                            .map_err(|error| error.to_string());
                        let _ = sender.send(WorkerMessage::Processed(result));
                    });
                }
                WorkerMessage::Decoded(Err(error)) => {
                    self.error = Some(error);
                    self.busy = None;
                }
                WorkerMessage::Processed(Ok(spectrum)) => {
                    self.spectrum = spectrum;
                    self.busy = None;
                }
                WorkerMessage::Processed(Err(error)) => {
                    self.error = Some(format!("Could not process audio: {error}"));
                    self.busy = None;
                }
                WorkerMessage::Exported(Ok(())) => {
                    self.exporting = false;
                    self.export_error = match self.export_controller.lock() {
                        Ok(mut controller) => controller.error.take(),
                        Err(_) => Some("Could not retrieve MP4 export status".to_owned()),
                    };
                    if self.export_error.is_none() {
                        let output = self
                            .export_output
                            .as_ref()
                            .map(|path| path.display().to_string())
                            .unwrap_or_else(|| "selected file".to_owned());
                        self.export_success = Some(format!("Saved full-track render: {output}"));
                    }
                }
                WorkerMessage::Exported(Err(error)) => {
                    self.exporting = false;
                    self.export_success = None;
                    let render_error = match self.export_controller.lock() {
                        Ok(mut controller) => controller.error.take(),
                        Err(_) => Some("Export render status is unavailable".to_owned()),
                    };
                    self.export_error = Some(match render_error {
                        Some(render_error) => {
                            format!("{render_error}; FFmpeg also failed: {error}")
                        }
                        None => error,
                    });
                }
            }
        }

        ui.ctx().request_repaint_after(Duration::from_millis(16));
        let audio_position_seconds = self.audio_position_seconds() as f32;

        let available_size = ui.available_size();
        ui.horizontal(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(292.0, available_size.y),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    egui::Frame::new()
                        .fill(egui::Color32::from_rgb(19, 23, 31))
                        .inner_margin(egui::Margin::same(18))
                        .show(ui, |ui| {
                            egui::ScrollArea::vertical().show(ui, |ui| {
                                ui.heading("SHOWVIEWER");
                                ui.label(
                                    egui::RichText::new("AUDIO-REACTIVE VISUALS")
                                        .small()
                                        .color(egui::Color32::from_rgb(139, 153, 173)),
                                );
                                ui.label(
                                    egui::RichText::new(
                                        "Sync playback, shape the look, export the full track.",
                                    )
                                    .small()
                                    .color(egui::Color32::from_rgb(139, 153, 173)),
                                );
                                ui.add_space(18.0);

                                ui.label(
                                    egui::RichText::new("AUDIO SOURCE")
                                        .small()
                                        .strong()
                                        .color(egui::Color32::from_rgb(139, 153, 173)),
                                );
                                ui.add_space(6.0);
                                if ui
                                    .add_enabled(
                                        self.busy.is_none() && !self.exporting,
                                        egui::Button::new("Open audio file")
                                            .min_size(egui::vec2(256.0, 34.0)),
                                    )
                                    .clicked()
                                    && let Some(file) = audio::open_file()
                                {
                                    self.error = None;
                                    if let Err(error) = self.pause_audio() {
                                        self.error = Some(error);
                                    }
                                    self.selected_file = Some(file.clone());
                                    self.audio_duration_seconds = 0.0;
                                    self.audio_sample_rate = 0;
                                    self.playback_position_seconds = 0.0;
                                    self.spectrum.clear();
                                    self.export_success = None;
                                    self.busy = Some(WorkerKind::Decoding);
                                    let sender = self.worker_tx.clone();
                                    let display_path = file.display().to_string();
                                    std::thread::spawn(move || {
                                        let result = audio::analyze(&file).map_err(|error| {
                                            format!("Could not analyze {display_path}: {error}")
                                        });
                                        let _ = sender.send(WorkerMessage::Decoded(result));
                                    });
                                }
                                if let Some(file) = &self.selected_file {
                                    ui.add_space(5.0);
                                    ui.label(
                                        egui::RichText::new(
                                            file.file_name().unwrap_or_default().to_string_lossy(),
                                        )
                                        .small()
                                        .color(egui::Color32::from_rgb(200, 208, 220)),
                                    );
                                }
                                ui.add_space(6.0);
                                ui.horizontal(|ui| {
                                    let audio_status = match self.busy {
                                        Some(WorkerKind::Decoding) => "Loading audio...",
                                        Some(WorkerKind::Processing) => "Analyzing audio...",
                                        None if self.playback_started.is_some() => "Playing",
                                        None if !self.spectrum.is_empty() => "Ready",
                                        None if self.error.is_some() => "Audio error",
                                        None => "No audio",
                                    };
                                    let status_color = if self.error.is_some() {
                                        egui::Color32::LIGHT_RED
                                    } else if !self.spectrum.is_empty() {
                                        egui::Color32::from_rgb(119, 207, 170)
                                    } else {
                                        egui::Color32::from_rgb(154, 164, 180)
                                    };
                                    ui.label(
                                        egui::RichText::new(audio_status)
                                            .small()
                                            .color(status_color),
                                    );
                                    if self.busy.is_some() {
                                        ui.spinner();
                                    }
                                });
                                if self.audio_duration_seconds > 0.0 {
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "{} · {} Hz · {} analyzed frames",
                                            format_time(self.audio_duration_seconds),
                                            self.audio_sample_rate,
                                            self.spectrum.len() / visuals::SPECTRUM_BINS
                                        ))
                                        .small()
                                        .color(egui::Color32::from_rgb(139, 153, 173)),
                                    );
                                }
                                ui.add_space(18.0);
                                ui.separator();
                                ui.add_space(12.0);

                                ui.label(
                                    egui::RichText::new("SCENE")
                                        .small()
                                        .strong()
                                        .color(egui::Color32::from_rgb(139, 153, 173)),
                                );
                                ui.add_space(5.0);
                                egui::ComboBox::from_id_salt("visual_scene")
                                    .selected_text(self.scene.label())
                                    .width(248.0)
                                    .show_ui(ui, |ui| {
                                        for scene in visuals::Scene::ALL {
                                            if ui
                                                .selectable_value(
                                                    &mut self.scene,
                                                    scene,
                                                    scene.label(),
                                                )
                                                .clicked()
                                            {
                                                self.select_scene(scene);
                                            }
                                        }
                                    });
                                let multi_band_selected =
                                    self.scene == visuals::Scene::MultiBandVj;
                                if ui
                                    .add_sized(
                                        [256.0, 34.0],
                                        egui::Button::new(
                                            egui::RichText::new("Multi-band VJ")
                                                .strong()
                                                .color(egui::Color32::WHITE),
                                        )
                                        .fill(if multi_band_selected {
                                            egui::Color32::from_rgb(63, 116, 150)
                                        } else {
                                            egui::Color32::from_rgb(46, 88, 119)
                                        }),
                                    )
                                    .on_hover_text(
                                        "Bass pulses · midrange warps geometry · treble adds spokes and sparkles",
                                    )
                                    .clicked()
                                {
                                    self.select_scene(visuals::Scene::MultiBandVj);
                                }
                                ui.add_space(8.0);
                                let toggle_label = if self.reactive_rendering {
                                    "Stop Shader Preview"
                                } else {
                                    "Start Shader Preview"
                                };
                                let toggle_button =
                                    egui::Button::new(egui::RichText::new(toggle_label).strong())
                                        .fill(if self.reactive_rendering {
                                            egui::Color32::from_rgb(74, 91, 103)
                                        } else {
                                            egui::Color32::from_rgb(46, 132, 117)
                                        })
                                        .min_size(egui::vec2(256.0, 38.0));
                                if ui
                                    .add_enabled(
                                        !self.exporting
                                            && (self.reactive_rendering
                                                || self.shader_pipeline.is_some()),
                                        toggle_button,
                                    )
                                    .clicked()
                                {
                                    self.reactive_rendering = !self.reactive_rendering;
                                    if self.reactive_rendering {
                                        if !self.scene.uses_shader() {
                                            self.scene = visuals::Scene::Reactive;
                                        } else if self.scene.is_vj_mode() {
                                            self.vj_seed = visuals::new_vj_seed();
                                        }
                                        self.preview_started = Instant::now();
                                        if self.selected_file.is_some()
                                            && self.busy.is_none()
                                            && self.playback_started.is_none()
                                            && (!self.scene.is_vj_mode() || self.vj_audio_reactive)
                                            && let Err(error) = self.play_audio()
                                        {
                                            self.error = Some(error);
                                        }
                                    } else if let Err(error) = self.pause_audio() {
                                        self.error = Some(error);
                                    }
                                }
                                if self.scene.is_vj_mode()
                                    && ui
                                        .add_enabled(
                                            !self.exporting,
                                            egui::Button::new("Generate new VJ composition")
                                                .min_size(egui::vec2(256.0, 32.0)),
                                        )
                                        .clicked()
                                {
                                    self.vj_seed = visuals::new_vj_seed();
                                    self.audio_smoother = visuals::AudioSmoother::default();
                                    self.preview_started = Instant::now();
                                }
                                if self.scene.is_vj_mode() {
                                    ui.add_space(8.0);
                                    ui.label(
                                        egui::RichText::new("LOOP DRIVE")
                                            .small()
                                            .strong()
                                            .color(egui::Color32::from_rgb(139, 153, 173)),
                                    );
                                    let mode_changed = ui
                                        .horizontal(|ui| {
                                            let audio_changed = ui
                                                .radio_value(
                                                    &mut self.vj_audio_reactive,
                                                    true,
                                                    "Audio reactive",
                                                )
                                                .changed();
                                            let autonomous_changed = ui
                                                .radio_value(
                                                    &mut self.vj_audio_reactive,
                                                    false,
                                                    "Self-running",
                                                )
                                                .changed();
                                            audio_changed || autonomous_changed
                                        })
                                        .inner;
                                    if mode_changed {
                                        self.preview_started = Instant::now();
                                        self.audio_smoother = visuals::AudioSmoother::default();
                                        if !self.vj_audio_reactive
                                            && let Err(error) = self.pause_audio()
                                        {
                                            self.error = Some(error);
                                        }
                                    }
                                    ui.label(
                                        egui::RichText::new(
                                            if self.scene == visuals::Scene::MultiBandVj {
                                                "Bass pulses · mids warp geometry · highs spark."
                                            } else if self.vj_audio_reactive {
                                                "Shape the loop with the selected track."
                                            } else {
                                                "A standalone 16-second loop; no audio needed."
                                            },
                                        )
                                        .small()
                                        .color(egui::Color32::from_rgb(139, 153, 173)),
                                    );
                                }
                                if let Some(error) = &self.shader_error {
                                    ui.add_space(4.0);
                                    ui.colored_label(egui::Color32::LIGHT_RED, error);
                                } else if self.reactive_rendering
                                    && let Some(warning) = &self.shader_warning
                                {
                                    ui.add_space(4.0);
                                    ui.colored_label(egui::Color32::YELLOW, warning);
                                }
                                ui.add_space(18.0);
                                ui.separator();
                                ui.add_space(12.0);

                                ui.label(
                                    egui::RichText::new("COLOR PALETTE")
                                        .small()
                                        .strong()
                                        .color(egui::Color32::from_rgb(139, 153, 173)),
                                );
                                ui.add_space(6.0);
                                ui.add_enabled_ui(!self.exporting, |ui| {
                                    ui.horizontal_wrapped(|ui| {
                                        for palette in Palette::PRESETS {
                                            let selected = self.palette == palette;
                                            let (primary, secondary) = palette
                                                .colors()
                                                .expect("preset palette has colors");
                                            ui.horizontal(|ui| {
                                                ui.label(egui::RichText::new("●").color(primary));
                                                ui.label(egui::RichText::new("●").color(secondary));
                                                if ui
                                                    .selectable_label(selected, palette.label())
                                                    .clicked()
                                                {
                                                    self.palette = palette;
                                                    self.primary_color = primary;
                                                    self.secondary_color = secondary;
                                                }
                                            });
                                        }
                                        if ui
                                            .selectable_label(
                                                self.palette == Palette::Custom,
                                                "Custom colors",
                                            )
                                            .clicked()
                                        {
                                            self.palette = Palette::Custom;
                                        }
                                    });
                                    ui.add_space(5.0);
                                    ui.horizontal(|ui| {
                                        ui.label("Primary");
                                        if ui
                                            .color_edit_button_srgba(&mut self.primary_color)
                                            .changed()
                                        {
                                            self.palette = Palette::Custom;
                                        }
                                        ui.add_space(8.0);
                                        ui.label("Secondary");
                                        if ui
                                            .color_edit_button_srgba(&mut self.secondary_color)
                                            .changed()
                                        {
                                            self.palette = Palette::Custom;
                                        }
                                    });
                                });
                                ui.add_space(18.0);
                                ui.separator();
                                ui.add_space(12.0);

                                ui.label(
                                    egui::RichText::new("EXPORT")
                                        .small()
                                        .strong()
                                        .color(egui::Color32::from_rgb(139, 153, 173)),
                                );
                                ui.add_space(5.0);
                                ui.add_enabled_ui(!self.exporting, |ui| {
                                    egui::ComboBox::from_id_salt("export_resolution")
                                        .selected_text(self.export_resolution.label())
                                        .width(248.0)
                                        .show_ui(ui, |ui| {
                                            for resolution in ExportResolution::ALL {
                                                ui.selectable_value(
                                                    &mut self.export_resolution,
                                                    resolution,
                                                    resolution.label(),
                                                );
                                            }
                                        });
                                });
                                ui.add_space(7.0);
                                let standalone_vj_loop =
                                    self.scene.is_vj_mode() && !self.vj_audio_reactive;
                                let export_label = if self.exporting {
                                    if standalone_vj_loop {
                                        "Rendering VJ loop..."
                                    } else {
                                        "Rendering full track..."
                                    }
                                } else if standalone_vj_loop {
                                    "Export 16-second VJ loop"
                                } else {
                                    "Export full audio render"
                                };
                                let export_disabled_reason = self.export_disabled_reason();
                                if ui
                                    .add_enabled(
                                        export_disabled_reason.is_none(),
                                        egui::Button::new(export_label)
                                            .min_size(egui::vec2(256.0, 36.0)),
                                    )
                                    .clicked()
                                    && let Some(output) = rfd::FileDialog::new()
                                        .add_filter("MP4 video", &["mp4"])
                                        .set_file_name(if self.scene.is_vj_mode() {
                                            if standalone_vj_loop {
                                                if self.scene == visuals::Scene::MultiBandVj {
                                                    "multiband-vj-loop.mp4"
                                                } else {
                                                    "vj-loop.mp4"
                                                }
                                            } else {
                                                if self.scene == visuals::Scene::MultiBandVj {
                                                    "multiband-vj-audio-reactive.mp4"
                                                } else {
                                                    "vj-audio-reactive.mp4"
                                                }
                                            }
                                        } else {
                                            "audio-reactive-render.mp4"
                                        })
                                        .save_file()
                                {
                                    self.start_export(output);
                                }
                                if self.exporting {
                                    match self.export_controller.lock() {
                                        Ok(controller) => {
                                            if let Some(capture) = &controller.capture {
                                                let progress = capture.next_frame as f32
                                                    / capture.frame_count as f32;
                                                ui.add(
                                                    egui::ProgressBar::new(progress)
                                                        .desired_width(256.0)
                                                        .show_percentage(),
                                                );
                                                ui.label(
                                                    egui::RichText::new(format!(
                                                        "{} / {} · frame {} of {}",
                                                        format_time(
                                                            capture.next_frame as f64
                                                                / f64::from(
                                                                    video_export::EXPORT_FPS
                                                                )
                                                        ),
                                                        format_time(
                                                            capture.frame_count as f64
                                                                / f64::from(
                                                                    video_export::EXPORT_FPS
                                                                )
                                                        ),
                                                        capture.next_frame.min(capture.frame_count),
                                                        capture.frame_count
                                                    ))
                                                    .small()
                                                    .color(egui::Color32::from_rgb(157, 170, 188)),
                                                );
                                            } else {
                                                ui.horizontal(|ui| {
                                                    ui.spinner();
                                                    ui.label("Finishing video encoding...");
                                                });
                                            }
                                        }
                                        Err(_) => {
                                            ui.colored_label(
                                                egui::Color32::LIGHT_RED,
                                                "Export status unavailable",
                                            );
                                        }
                                    }
                                } else if let Some(reason) = export_disabled_reason {
                                    ui.label(
                                        egui::RichText::new(reason)
                                            .small()
                                            .color(egui::Color32::from_rgb(184, 164, 128)),
                                    );
                                }
                                let export_description = if standalone_vj_loop {
                                    format!(
                                        "Standalone seamless VJ loop · H.264 · 30 fps · {}",
                                        format_time(f64::from(visuals::LOOP_SECONDS))
                                    )
                                } else {
                                    format!(
                                        "Reactive video + source audio · H.264 + AAC · 30 fps · {}",
                                        if self.audio_duration_seconds > 0.0 {
                                            format_time(self.audio_duration_seconds)
                                        } else {
                                            "load audio to enable export".to_owned()
                                        }
                                    )
                                };
                                ui.label(
                                    egui::RichText::new(export_description)
                                        .small()
                                        .color(egui::Color32::from_rgb(139, 153, 173)),
                                );
                                if self.shader_pipeline.is_none() {
                                    ui.colored_label(
                                        egui::Color32::LIGHT_RED,
                                        "Reactive rendering is unavailable on this system.",
                                    );
                                }
                                ui.label(
                                    egui::RichText::new("Requires FFmpeg with libx264 on PATH.")
                                        .small()
                                        .color(egui::Color32::from_rgb(139, 153, 173)),
                                );
                                if let Some(error) = &self.export_error {
                                    ui.add_space(4.0);
                                    ui.colored_label(egui::Color32::LIGHT_RED, error);
                                }
                                if let Some(success) = &self.export_success {
                                    ui.add_space(4.0);
                                    ui.colored_label(
                                        egui::Color32::from_rgb(119, 207, 170),
                                        success,
                                    );
                                }
                                if let Some(error) = &self.error {
                                    ui.add_space(4.0);
                                    ui.colored_label(egui::Color32::LIGHT_RED, error);
                                }
                            });
                        });
                },
            );
            ui.separator();

            ui.allocate_ui_with_layout(
                ui.available_size(),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    egui::Frame::new()
                        .fill(egui::Color32::from_rgb(13, 16, 22))
                        .inner_margin(egui::Margin::same(22))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.heading("Preview");
                                ui.add_space(8.0);
                                ui.label(
                                    egui::RichText::new(self.scene.label())
                                        .color(egui::Color32::from_rgb(157, 170, 188)),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if self.exporting {
                                            ui.colored_label(
                                                egui::Color32::from_rgb(237, 191, 111),
                                                "EXPORTING",
                                            );
                                        } else if self.playback_started.is_some() {
                                            ui.colored_label(
                                                egui::Color32::from_rgb(119, 207, 170),
                                                "● PLAYING",
                                            );
                                        } else if self.scene == visuals::Scene::Reactive
                                            && self.reactive_rendering
                                        {
                                            ui.label(
                                                egui::RichText::new("REACTIVE PREVIEW")
                                                    .small()
                                                    .color(egui::Color32::from_rgb(139, 153, 173)),
                                            );
                                        } else {
                                            ui.label(
                                                egui::RichText::new("PREVIEW")
                                                    .small()
                                                    .color(egui::Color32::from_rgb(139, 153, 173)),
                                            );
                                        }
                                    },
                                );
                            });
                            ui.add_space(12.0);

                            let available = ui.available_rect_before_wrap();
                            let preview_area = egui::Rect::from_min_max(
                                available.min,
                                egui::pos2(
                                    available.max.x,
                                    (available.max.y - 48.0).max(available.min.y),
                                ),
                            );
                            let frame_rect = egui::Rect::from_center_size(
                                preview_area.center(),
                                egui::vec2(
                                    preview_area.width().min(preview_area.height() * 16.0 / 9.0),
                                    preview_area.height().min(preview_area.width() * 9.0 / 16.0),
                                ),
                            );
                            ui.painter().rect_filled(
                                frame_rect.expand(7.0),
                                12.0,
                                egui::Color32::from_rgb(27, 32, 42),
                            );
                            ui.painter().rect_filled(
                                frame_rect,
                                6.0,
                                egui::Color32::from_rgb(7, 10, 17),
                            );
                            let visual_time = if self.scene.is_vj_mode() && !self.vj_audio_reactive
                            {
                                self.preview_started.elapsed().as_secs_f32()
                            } else if self.audio_duration_seconds > 0.0 {
                                audio_position_seconds
                            } else {
                                self.preview_started.elapsed().as_secs_f32()
                            };
                            visuals::render(
                                &ui.painter_at(frame_rect),
                                frame_rect,
                                &self.spectrum,
                                visual_time,
                                self.audio_duration_seconds as f32,
                                self.scene,
                                if self.reactive_rendering {
                                    self.shader_pipeline.as_ref()
                                } else {
                                    None
                                },
                                &mut self.audio_smoother,
                                ui.input(|input| input.stable_dt),
                                self.vj_seed,
                                self.vj_audio_reactive,
                                self.primary_color,
                                self.secondary_color,
                                Arc::clone(&self.export_controller),
                            );
                            if self.selected_file.is_none()
                                && !(self.scene.is_vj_mode() && !self.vj_audio_reactive)
                            {
                                ui.painter().text(
                                    frame_rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    "Load an audio file to begin",
                                    egui::FontId::proportional(20.0),
                                    egui::Color32::from_rgb(175, 187, 201),
                                );
                            } else if self.busy.is_some() {
                                let status = match self.busy {
                                    Some(WorkerKind::Decoding) => "Loading audio…",
                                    Some(WorkerKind::Processing) => "Analyzing audio…",
                                    None => unreachable!(),
                                };
                                ui.painter().text(
                                    frame_rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    status,
                                    egui::FontId::proportional(18.0),
                                    egui::Color32::from_rgb(175, 187, 201),
                                );
                            } else if self.spectrum.is_empty()
                                && !(self.scene.is_vj_mode() && !self.vj_audio_reactive)
                            {
                                ui.painter().text(
                                    frame_rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    "Audio analysis unavailable",
                                    egui::FontId::proportional(18.0),
                                    egui::Color32::from_rgb(175, 187, 201),
                                );
                            }
                            ui.allocate_rect(preview_area, egui::Sense::hover());
                            ui.add_space(5.0);
                            let mut requested_position = audio_position_seconds as f64;
                            ui.horizontal(|ui| {
                                let playing = self.playback_started.is_some();
                                if ui
                                    .add_enabled(
                                        self.audio_duration_seconds > 0.0
                                            && self.busy.is_none()
                                            && !self.exporting,
                                        egui::Button::new(if playing { "Pause" } else { "Play" })
                                            .min_size(egui::vec2(64.0, 30.0)),
                                    )
                                    .clicked()
                                {
                                    if playing {
                                        if let Err(error) = self.pause_audio() {
                                            self.error = Some(error);
                                        }
                                    } else if let Err(error) = self.play_audio() {
                                        self.error = Some(error);
                                    }
                                }

                                let slider_width = (ui.available_width() - 116.0).max(80.0);
                                let response = ui
                                    .add_enabled_ui(
                                        self.audio_duration_seconds > 0.0 && !self.exporting,
                                        |ui| {
                                            ui.add_sized(
                                                [slider_width, 24.0],
                                                egui::Slider::new(
                                                    &mut requested_position,
                                                    0.0..=self.audio_duration_seconds.max(0.01),
                                                )
                                                .show_value(false)
                                                .trailing_fill(true)
                                                .fixed_decimals(1),
                                            )
                                        },
                                    )
                                    .inner;
                                if response.changed() {
                                    let was_playing = self.playback_started.is_some();
                                    if let Err(error) = self.pause_audio() {
                                        self.error = Some(error);
                                    }
                                    self.playback_position_seconds = requested_position;
                                    self.audio_smoother = visuals::AudioSmoother::default();
                                    if was_playing && let Err(error) = self.play_audio() {
                                        self.error = Some(error);
                                    }
                                }
                                ui.label(format!(
                                    "{} / {}",
                                    format_time(requested_position),
                                    format_time(self.audio_duration_seconds)
                                ));
                            });
                            ui.label(
                                egui::RichText::new(
                                    "Play starts the selected visual with synchronized audio.",
                                )
                                .small()
                                .color(egui::Color32::from_rgb(139, 153, 173)),
                            );
                        });
                },
            );
        });
    }
}

fn format_time(seconds: f64) -> String {
    if !seconds.is_finite() || seconds < 0.0 {
        return "0:00".to_owned();
    }
    let total_seconds = seconds.floor() as u64;
    format!("{}:{:02}", total_seconds / 60, total_seconds % 60)
}
