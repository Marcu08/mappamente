//! GUI-subsystem entry point for double-click launch.
//!
//! On Windows release builds this binary has no console window. CLI users
//! should use `mappamente.exe` instead.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::path::PathBuf;

fn main() {
    let initial = std::env::args_os().nth(1).map(PathBuf::from);
    if let Err(error) = mappamente::gui::run(initial) {
        rfd::MessageDialog::new()
            .set_title("Mappamente")
            .set_description(error.to_string())
            .set_level(rfd::MessageLevel::Error)
            .show();
    }
}
