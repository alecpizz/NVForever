//! Messages from wgpu and the Vulkan validation layer.

use std::sync::Mutex;

/// What wgpu and the validation layer say.
static MESSAGES: Mutex<Vec<(log::Level, String)>> = Mutex::new(Vec::new());

struct Log;
static LOG: Log = Log;

impl log::Log for Log {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= log::Level::Warn || m.target().starts_with("wgpu_hal::vulkan::instance")
    }

    fn log(&self, r: &log::Record) {
        // The instance's lines of debug level hold whether the layer was found, but also much else.
        if !self.enabled(r.metadata()) {
            return;
        }
        let text = r.args().to_string();
        if r.level() <= log::Level::Warn || text.contains("unable to find layer") {
            if let Ok(mut m) = MESSAGES.lock() {
                m.push((r.level(), format!("{}: {text}", r.target())));
            }
        }
    }

    fn flush(&self) {}
}

/// Whether nothing was said that is an error, and the layer was there when it was asked for.
pub fn quiet(validate: bool) -> bool {
    let messages = MESSAGES.lock().map(|m| m.clone()).unwrap_or_default();
    for (level, text) in &messages {
        println!("{level} {text}");
    }
    let missing = messages.iter().any(|m| m.1.contains("unable to find layer"));
    let errors = messages.iter().filter(|m| m.0 == log::Level::Error).count();
    if validate {
        println!("VALIDATION: {}; {errors} errors", if missing { "the layer is NOT installed, nothing was validated" } else { "the layer was on" });
    }
    errors == 0 && !(validate && missing)
}

pub fn listen() {
    if log::set_logger(&LOG).is_ok() {
        log::set_max_level(log::LevelFilter::Debug);
    }
}
