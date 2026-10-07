pub mod audio;

use eframe::egui;
use std::path::PathBuf;

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
                self.selected_file = audio::openFile();
            }
            if let Some(file) = &self.selected_file {
                ui.label(format!("File selected: {}", file.display()));
            } else {
                ui.label("No file selected");
            }
        });
    }
}


fn loadfile() {
    audio::load();
}