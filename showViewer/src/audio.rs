use rfd::FileDialog;
use std::path::PathBuf;
// import the audio parsing libraries



pub fn load() {
    println!("file loading");


}

pub fn openFile() -> Option<PathBuf>{
    let files: Option<PathBuf> = FileDialog::new()
        .add_filter("audio", &["mp3", "wav", "flac"])
        .set_directory("/")
        .pick_file();

    files
}

