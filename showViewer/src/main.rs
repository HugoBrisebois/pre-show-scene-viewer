pub mod audio;

use eframe::egui;
use std::path::{Path, PathBuf};

fn main() {
    // init the window
    let native_options = eframe::NativeOptions::default();
    eframe::run_native("ShowViewer", native_options, Box::new(|cc| Ok(Box::new(ShowViewer::new(cc)))));
}

#[derive(Default)]
struct ShowViewer {
    selected_file: Option<PathBuf>,
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
                if let Some(file) = audio::openFile() {
                    self.selected_file = Some(file.clone());

                    if let Err(error) = start_analysis(&file) {
                        eprintln!("Could not analyze {}: {error}", file.display());
                    }
                }
            }
            if let Some(file) = &self.selected_file {
                ui.label(format!("File selected: {}", file.display()));
            } else {
                ui.label("No file selected");
            }
        });
    }
}

fn start_analysis(file: &Path) -> Result<(), Box<dyn std::error::Error>> {
    audio::analyze(file)
}