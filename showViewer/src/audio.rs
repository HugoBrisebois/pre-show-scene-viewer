use rfd::FileDialog;
use std::fs::File;
use std::sync::Arc;


pub fn load() {
    println!("file loading");
}

pub fn openFile() {
    let _files = FileDialog::new()
        .add_filter("Audio", &["mp3", "wav", "flac" ])
        .set_directory("/")
        .pick_file();
}