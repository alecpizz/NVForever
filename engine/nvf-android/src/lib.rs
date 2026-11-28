//! The Android shared-library entry point; desktop builds keep their ordinary binary.
#[cfg(target_os = "android")]
#[bevy::prelude::bevy_main]
fn main() {
    nvforever::run_android();
}

#[cfg(target_os = "android")]
mod launcher;
