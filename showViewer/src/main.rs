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
                                    let analyze_label = match self.busy {
                                        Some(WorkerKind::Decoding) => "Decoding audio...",
                                        Some(WorkerKind::Processing) => "Analyzing...",
                                        None => "Analyze audio",
                                    };
                                    if ui
                                        .add_enabled(
                                            self.busy.is_none()
                                                && !self.exporting
                                                && !self.audio_samples.is_empty(),
                                            egui::Button::new(analyze_label),
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
                                    let audio_status = match self.busy {
                                        Some(WorkerKind::Decoding) => "Loading",
                                        Some(WorkerKind::Processing) => "Working",
                                        None if !self.spectrum.is_empty() => "Ready",
                                        None if !self.audio_samples.is_empty() => "Loaded",
                                        None => "No audio",
                                    };
                                    let status_color = if !self.spectrum.is_empty() {
                                        egui::Color32::from_rgb(119, 207, 170)
                                    } else {
                                        egui::Color32::from_rgb(154, 164, 180)
                                    };
                                    ui.label(
                                        egui::RichText::new(audio_status)
                                            .small()
                                            .color(status_color),
                                    );
                                });
                                if !self.spectrum.is_empty() {
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "{} analyzed frames",
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
                                                && scene != visuals::Scene::Reactive
                                            {
                                                self.reactive_rendering = false;
                                            }
                                        }
                                    });
                                ui.add_space(8.0);
                                let toggle_label = if self.reactive_rendering {
                                    "Stop Reactive Preview"
                                } else {
                                    "Start Reactive Preview"
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
                                        self.scene = visuals::Scene::Reactive;
                                        self.preview_started = Instant::now();
                                    }
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
                                let export_label = if self.exporting {
                                    "Rendering MP4..."
                                } else {
                                    "Export 16-second loop"
                                };
                                if ui
                                    .add_enabled(
                                        !self.exporting && self.shader_pipeline.is_some(),
                                        egui::Button::new(export_label)
                                            .min_size(egui::vec2(256.0, 36.0)),
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
                                                let progress = capture.next_frame as f32
                                                    / capture.frame_count as f32;
                                                ui.add(
                                                    egui::ProgressBar::new(progress)
                                                        .desired_width(256.0)
                                                        .show_percentage(),
                                                );
                                            } else {
                                                ui.label("Finishing video encoding...");
                                            }
                                        }
                                        Err(_) => {
                                            ui.colored_label(
                                                egui::Color32::LIGHT_RED,
                                                "Export status unavailable",
                                            );
                                        }
                                    }
                                }
                                ui.label(
                                    egui::RichText::new("Silent MP4 · 30 fps · Requires FFmpeg")
                                        .small()
                                        .color(egui::Color32::from_rgb(139, 153, 173)),
                                );
                                if let Some(error) = &self.export_error {
                                    ui.add_space(4.0);
                                    ui.colored_label(egui::Color32::LIGHT_RED, error);
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
                                        } else if self.scene == visuals::Scene::Reactive
                                            && self.reactive_rendering
                                        {
                                            ui.colored_label(
                                                egui::Color32::from_rgb(119, 207, 170),
                                                "● LIVE",
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
                            let frame_rect = egui::Rect::from_center_size(
                                available.center(),
                                egui::vec2(
                                    available.width().min(available.height() * 16.0 / 9.0),
                                    available.height().min(available.width() * 9.0 / 16.0),
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
                            visuals::render(
                                &ui.painter_at(frame_rect),
                                frame_rect,
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
                            ui.allocate_rect(available, egui::Sense::hover());
                        });
                },
            );
        });
    }
}

fn start_analysis(file: &Path) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
    audio::analyze(file)
}
