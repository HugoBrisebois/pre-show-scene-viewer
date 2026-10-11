pub mod audio;
pub mod video_export;
pub mod visuals;

use eframe::egui;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const FFT_BLOCK_SIZE: usize = 1024;

fn main() -> eframe::Result<()> {
    // init the window
    let native_options = eframe::NativeOptions {
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_native(
        "ShowViewer",
        native_options,
        Box::new(|cc| Ok(Box::new(ShowViewer::new(cc)))),
    )
}

struct ShowViewer {
    selected_file: Option<PathBuf>,
    audio_samples: Arc<[f32]>,
    spectrum: Vec<f32>,
    error: Option<String>,
    worker_tx: Sender<WorkerMessage>,
    worker_rx: Receiver<WorkerMessage>,
    busy: Option<WorkerKind>,
    scene: visuals::Scene,
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
}

enum WorkerMessage {
    Decoded(Result<Vec<f32>, String>),
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
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
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
            audio_samples: Arc::from([]),
            spectrum: Vec::new(),
            error: None,
            worker_tx,
            worker_rx,
            busy: None,
            scene: visuals::Scene::Reactive,
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
        }
    }

    fn start_export(&mut self, output: PathBuf) {
        let output = output.with_extension("mp4");
        let (width, height) = self.export_resolution.dimensions();
        let (completion_sender, completion_receiver) = mpsc::channel();
        let frame_sender =
            video_export::start_mp4_encoder(&output, width, height, completion_sender);
        let worker_sender = self.worker_tx.clone();
        std::thread::spawn(move || {
            let result = completion_receiver
                .recv()
                .unwrap_or_else(|_| Err("MP4 encoder exited without a result".to_owned()));
            let _ = worker_sender.send(WorkerMessage::Exported(result));
        });

        let audio_controls = visuals::export_audio_controls(
            &self.spectrum,
            video_export::EXPORT_FRAME_COUNT,
            video_export::EXPORT_FPS,
        );
        let capture = visuals::ExportCapture {
            width,
            height,
            frame_count: video_export::EXPORT_FRAME_COUNT,
            next_frame: 0,
            audio_controls,
            primary_color: self.primary_color,
            secondary_color: self.secondary_color,
            frame_sender,
        };

        match self.export_controller.lock() {
            Ok(mut controller) => {
                controller.capture = Some(capture);
                controller.error = None;
                self.export_error = None;
                self.exporting = true;
                self.scene = visuals::Scene::Reactive;
                self.reactive_rendering = true;
                self.preview_started = Instant::now();
            }
            Err(_) => {
                self.export_error =
                    Some("Could not start export: render queue is unavailable".into());
            }
        }
    }
}

impl eframe::App for ShowViewer {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        while let Ok(message) = self.worker_rx.try_recv() {
            match message {
                WorkerMessage::Decoded(Ok(samples)) => {
                    self.audio_samples = Arc::from(samples);
                    self.busy = None;
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
                }
                WorkerMessage::Exported(Err(error)) => {
                    self.exporting = false;
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

        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("Reactive VJ Loops");
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        self.busy.is_none() && !self.exporting,
                        egui::Button::new("Open audio"),
                    )
                    .clicked()
                {
                    if let Some(file) = audio::open_file() {
                        self.selected_file = Some(file.clone());
                        self.audio_samples = Arc::from([]);
                        self.spectrum.clear();
                        self.error = None;
                        self.busy = Some(WorkerKind::Decoding);
                        let sender = self.worker_tx.clone();
                        let display_path = file.display().to_string();
                        std::thread::spawn(move || {
                            let result = start_analysis(&file).map_err(|error| {
                                format!("Could not analyze {display_path}: {error}")
                            });
                            let _ = sender.send(WorkerMessage::Decoded(result));
                        });
                    }
                }
                if ui
                    .add_enabled(
                        self.busy.is_none() && !self.exporting && !self.audio_samples.is_empty(),
                        egui::Button::new("Analyze audio"),
                    )
                    .clicked()
                {
                    self.busy = Some(WorkerKind::Processing);
                    self.error = None;
                    let sender = self.worker_tx.clone();
                    let samples = Arc::clone(&self.audio_samples);
                    std::thread::spawn(move || {
                        let result = audio::process(&samples, FFT_BLOCK_SIZE)
                            .map_err(|error| error.to_string());
                        let _ = sender.send(WorkerMessage::Processed(result));
                    });
                }

                ui.add_enabled_ui(!self.exporting, |ui| {
                    egui::ComboBox::from_id_salt("visual_scene")
                        .selected_text(self.scene.label())
                        .show_ui(ui, |ui| {
                            for scene in visuals::Scene::ALL {
                                if ui
                                    .selectable_value(&mut self.scene, scene, scene.label())
                                    .clicked()
                                    && scene != visuals::Scene::Reactive
                                {
                                    self.reactive_rendering = false;
                                }
                            }
                        });
                });
            });

            ui.horizontal(|ui| {
                let toggle_button = egui::Button::new(if self.reactive_rendering {
                    "Stop Reactive Rendering"
                } else {
                    "Start Reactive Rendering"
                });
                if ui
                    .add_enabled(
                        !self.exporting
                            && (self.reactive_rendering || self.shader_pipeline.is_some()),
                        toggle_button,
                    )
                    .clicked()
                {
                    self.reactive_rendering = !self.reactive_rendering;
                    if self.reactive_rendering {
                        self.scene = visuals::Scene::Reactive;
                        self.preview_started = Instant::now();
                    }
                }
                if self.reactive_rendering {
                    ui.label("Reactive rendering is running");
                } else if self.shader_pipeline.is_some() {
                    ui.label("Reactive rendering is stopped");
                } else {
                    ui.colored_label(egui::Color32::RED, "Reactive shader is unavailable");
                }
            });
            if let Some(error) = &self.shader_error {
                ui.colored_label(egui::Color32::RED, error);
            }
            if self.reactive_rendering
                && let Some(warning) = &self.shader_warning
            {
                ui.colored_label(egui::Color32::YELLOW, warning);
            }

            if self.scene == visuals::Scene::Reactive {
                ui.add_enabled_ui(!self.exporting, |ui| {
                    ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt("reactive_palette")
                        .selected_text(self.palette.label())
                        .show_ui(ui, |ui| {
                            for palette in Palette::PRESETS {
                                if ui
                                    .selectable_value(&mut self.palette, palette, palette.label())
                                    .clicked()
                                    && let Some((primary, secondary)) = palette.colors()
                                {
                                    self.primary_color = primary;
                                    self.secondary_color = secondary;
                                }
                            }
                            ui.selectable_value(
                                &mut self.palette,
                                Palette::Custom,
                                Palette::Custom.label(),
                            );
                        });
                    ui.label("Primary");
                    if ui
                        .color_edit_button_srgba(&mut self.primary_color)
                        .changed()
                    {
                        self.palette = Palette::Custom;
                    }
                    ui.label("Secondary");
                    if ui
                        .color_edit_button_srgba(&mut self.secondary_color)
                        .changed()
                    {
                        self.palette = Palette::Custom;
                    }
                    });
                });
            }

            ui.horizontal(|ui| {
                ui.add_enabled_ui(!self.exporting, |ui| {
                    egui::ComboBox::from_id_salt("export_resolution")
                        .selected_text(self.export_resolution.label())
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

                let export_label = if self.exporting {
                    "Exporting Reactive MP4..."
                } else {
                    "Export 16s Reactive MP4"
                };
                if ui
                    .add_enabled(
                        !self.exporting && self.shader_pipeline.is_some(),
                        egui::Button::new(export_label),
                    )
                    .clicked()
                    && let Some(output) = rfd::FileDialog::new()
                        .add_filter("MP4 video", &["mp4"])
                        .set_file_name("reactive-loop.mp4")
                        .save_file()
                {
                    self.start_export(output);
                }

                if self.exporting {
                    match self.export_controller.lock() {
                        Ok(controller) => {
                            if let Some(capture) = &controller.capture {
                                let progress =
                                    capture.next_frame as f32 / capture.frame_count as f32;
                                ui.add(egui::ProgressBar::new(progress).show_percentage());
                            } else {
                                ui.label("Finishing MP4 encoding...");
                            }
                        }
                        Err(_) => {
                            ui.colored_label(egui::Color32::RED, "Export status unavailable");
                        }
                    }
                }
            });
            ui.label(
                "Exports a silent 16-second MP4 at 30 fps; analyzed audio drives the visuals. Requires FFmpeg.",
            );
            if let Some(error) = &self.export_error {
                ui.colored_label(egui::Color32::RED, error);
            }

            ui.horizontal(|ui| {
                if let Some(file) = &self.selected_file {
                    ui.label(file.file_name().unwrap_or_default().to_string_lossy());
                } else {
                    ui.label("Choose an audio file to drive the visuals");
                }
                match self.busy {
                    Some(WorkerKind::Decoding) => ui.label("Decoding audio..."),
                    Some(WorkerKind::Processing) => ui.label("Analyzing spectrum..."),
                    None if !self.spectrum.is_empty() => ui.label(format!(
                        "{} spectrum frames",
                        self.spectrum.len() / visuals::SPECTRUM_BINS
                    )),
                    None => ui.label("Preview loop is running"),
                };
            });

            let (preview_rect, _) = ui.allocate_at_least(
                ui.available_size_before_wrap()
                    .max(egui::vec2(360.0, 280.0)),
                egui::Sense::hover(),
            );
            visuals::render(
                &ui.painter_at(preview_rect),
                preview_rect,
                &self.spectrum,
                self.preview_started.elapsed().as_secs_f32(),
                self.scene,
                if self.reactive_rendering {
                    self.shader_pipeline.as_ref()
                } else {
                    None
                },
                &mut self.audio_smoother,
                ui.input(|input| input.stable_dt),
                self.primary_color,
                self.secondary_color,
                Arc::clone(&self.export_controller),
            );
            if let Some(error) = &self.error {
                ui.colored_label(egui::Color32::RED, error);
            }
        });
    }
}

fn start_analysis(file: &Path) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
    audio::analyze(file)
}
