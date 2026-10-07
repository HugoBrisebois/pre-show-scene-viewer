pub mod audio;

use eframe::egui;

fn main() {
    // init the window
    let native_options = eframe::NativeOptions::default();
    eframe::run_native("ShowViewer", native_options, Box::new(|cc| Ok(Box::new(ShowViewer::new(cc)))));
}

#[derive(Default)]
struct ShowViewer {}

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
            let file = String::new();
            if ui.button("OpenFile").clicked() {
                audio::openFile();
            }
            ui.label(format!("File loaded: {file}"));
        });
    }
}


fn loadfile() {
    audio::load();
}