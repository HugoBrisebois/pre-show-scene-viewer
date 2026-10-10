pub mod audio;

use eframe::egui;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

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

        if self.busy.is_some() {
            ui.ctx().request_repaint_after(Duration::from_millis(100));
        }

        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("Audio Analyzer");
            if ui
                .add_enabled(self.busy.is_none(), egui::Button::new("OpenFile"))
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
                        let result = start_analysis(&file)
                            .map_err(|error| format!("Could not analyze {display_path}: {error}"));
                        let _ = sender.send(WorkerMessage::Decoded(result));
                    });
                }
            }
            if let Some(file) = &self.selected_file {
                ui.label(format!("File selected: {}", file.display()));
            } else {
                ui.label("No file selected");
            }
            ui.label(format!("Decoded samples: {}", self.audio_samples.len()));
            if ui
                .add_enabled(
                    self.busy.is_none() && !self.audio_samples.is_empty(),
                    egui::Button::new("Start Analysis"),
                )
                .clicked()
            {
                self.busy = Some(WorkerKind::Processing);
                self.error = None;
                let sender = self.worker_tx.clone();
                let samples = Arc::clone(&self.audio_samples);
                std::thread::spawn(move || {
                    let result =
                        audio::process(&samples, FFT_BLOCK_SIZE).map_err(|error| error.to_string());
                    let _ = sender.send(WorkerMessage::Processed(result));
                });
            }
            match self.busy {
                Some(WorkerKind::Decoding) => ui.label("Decoding audio..."),
                Some(WorkerKind::Processing) => ui.label("Processing audio..."),
                None => ui.label(""),
            };
            ui.label(format!("Spectrum bins: {}", self.spectrum.len()));
            if let Some(error) = &self.error {
                ui.colored_label(egui::Color32::RED, error);
            }
        });
    }
}

fn start_analysis(file: &Path) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
    audio::analyze(file)
}
