use std::default;
use eframe::egui;
use rfd::FileDialog;

// defining the audio file structure
struct file {
    FileLocation: String,
    file: String
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
    let files = FileDialog::new()
        .add_filter("Audio", &["mp3", "wav", "flac" ])
        .set_directory("/")
        .pick_file();
}
