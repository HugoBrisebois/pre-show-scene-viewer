pub mod audio;
pub mod visuals;

use eframe::egui;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

const FFT_BLOCK_SIZE: usize = 1024;

fn main() -> eframe::Result<()> {
    // init the window
    let native_options = eframe::NativeOptions::default();
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
}

enum WorkerMessage {
    Decoded(Result<Vec<f32>, String>),
    Processed(Result<Vec<f32>, String>),
}

#[derive(Clone, Copy)]
enum WorkerKind {
    Decoding,
    Processing,
}

impl ShowViewer {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let (worker_tx, worker_rx) = mpsc::channel();
        Self {
            selected_file: None,
            audio_samples: Arc::from([]),
            spectrum: Vec::new(),
            error: None,
            worker_tx,
            worker_rx,
            busy: None,
            scene: visuals::Scene::Aurora,
            preview_started: Instant::now(),
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
            }
        }

        ui.ctx().request_repaint_after(Duration::from_millis(16));

        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("Reactive VJ Loops");
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(self.busy.is_none(), egui::Button::new("Open audio"))
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
                        self.busy.is_none() && !self.audio_samples.is_empty(),
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

                egui::ComboBox::from_id_salt("visual_scene")
                    .selected_text(self.scene.label())
                    .show_ui(ui, |ui| {
                        for scene in visuals::Scene::ALL {
                            ui.selectable_value(&mut self.scene, scene, scene.label());
                        }
                    });
            });

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
