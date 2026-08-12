use std::path::PathBuf;

#[cfg(target_os = "macos")]
pub fn choose_sample_directory() -> Result<Option<PathBuf>, String> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSModalResponseOK, NSOpenPanel};
    use objc2_foundation::NSString;

    let mtm = MainThreadMarker::new()
        .ok_or_else(|| "The sample folder chooser must run on the macOS UI thread.".to_string())?;
    let panel = NSOpenPanel::openPanel(mtm);
    panel.setCanChooseDirectories(true);
    panel.setCanChooseFiles(false);
    panel.setAllowsMultipleSelection(false);
    panel.setCanCreateDirectories(true);
    panel.setTitle(Some(&NSString::from_str(
        "Choose where Rippr saves WAV files",
    )));
    panel.setPrompt(Some(&NSString::from_str("Choose Folder")));
    if panel.runModal() != NSModalResponseOK {
        return Ok(None);
    }
    let path = panel
        .URL()
        .and_then(|url| url.path())
        .map(|path| PathBuf::from(path.to_string()))
        .ok_or_else(|| "The selected folder could not be resolved.".to_string())?;
    Ok(Some(path))
}

#[cfg(not(target_os = "macos"))]
pub fn choose_sample_directory() -> Result<Option<PathBuf>, String> {
    Err("The native sample folder chooser is currently available on macOS only.".into())
}
