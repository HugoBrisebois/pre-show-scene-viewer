pub mod audio;

use eframe::egui;
use std::path::{Path, PathBuf};

fn main() -> eframe::Result<()> {
    // init the window
    let native_options = eframe::NativeOptions::default();
    eframe::run_native(
        "ShowViewer",
        native_options,
        Box::new(|cc| Ok(Box::new(ShowViewer::new(cc)))),
    )
}

#[derive(Default)]
struct ShowViewer {
    selected_file: Option<PathBuf>,
    audio_samples: Vec<f32>,
}

impl ShowViewer {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        // TODO: customize egui here
        Self::default()
    }
}

impl eframe::App for ShowViewer {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("Audio Analyzer");
            if ui.button("OpenFile").clicked() {
                if let Some(file) = audio::open_file() {
                    self.selected_file = Some(file.clone());

                    match start_analysis(&file) {
                        Ok(samples) => self.audio_samples = samples,
                        Err(error) => eprintln!("Could not analyze {}: {error}", file.display()),
                    }
                }
            }
            if let Some(file) = &self.selected_file {
                ui.label(format!("File selected: {}", file.display()));
            } else {
                ui.label("No file selected");
            }
            ui.label(format!("Decoded samples: {}", self.audio_samples.len()));
        });
    }
}

fn start_analysis(file: &Path) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
    audio::analyze(file)
}
