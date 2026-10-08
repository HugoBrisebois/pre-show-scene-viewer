use rfd::FileDialog;
use std::path::PathBuf;
use std::{fs::File, path::Path};

pub fn openFile() -> Option<PathBuf>{
    let files: Option<PathBuf> = FileDialog::new()
        .add_filter("audio", &["mp3", "wav", "flac"])
        .set_directory("/")
        .pick_file();

    files
}

pub fn analyze(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let file = File::open(path)?;

    // Decode and analyze `file` here (for example, using Symphonia).
    Ok(())
}