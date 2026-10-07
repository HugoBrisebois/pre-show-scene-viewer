pub mod audio;

use eframe::egui;
use rfd::FileDialog;
use std::fs::File;
use std::sync::Arc;

// use another file for loading and proccessing the audio files


// defining global variables
struct filepath {
    filename : String,
    filetype : String,
}

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
            if ui.button("OpenFile").clicked() {
                let mut  file = openFile();
            }
            ui.label(format!("File loaded: ", ));
        });
    }
}

fn openFile() {
    let _files = FileDialog::new()
        .add_filter("Audio", &["mp3", "wav", "flac" ])
        .set_directory("/")
        .pick_file();
}


fn loadfile() {
    audio::load();
}