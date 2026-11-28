//! Android platform boundary. See docs/design/android.md; game files stay read-only.
use jni::{objects::JObject, JavaVM};

pub(crate) fn activity_string(method: &str, argument: Option<&str>) -> Result<String, String> {
    let app = bevy::android::ANDROID_APP
        .get()
        .ok_or("Android activity is not ready")?;
    // SAFETY: AndroidApp owns the VM and activity global reference for this call.
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) }.map_err(|e| e.to_string())?;
    let mut env = vm.attach_current_thread().map_err(|e| e.to_string())?;
    // Polling controller metadata runs repeatedly on a possibly already-attached
    // thread. Bound every call's local JNI references, including the returned String.
    let result = env.with_local_frame(16, |env| -> Result<String, jni::errors::Error> {
        let activity = unsafe { JObject::from_raw(app.activity_as_ptr().cast()) };
        let value = match argument {
            Some(text) => {
                let text = env.new_string(text)?;
                env.call_method(
                    &activity,
                    method,
                    "(Ljava/lang/String;)Ljava/lang/String;",
                    &[(&text).into()],
                )
            }
            None => env.call_method(&activity, method, "()Ljava/lang/String;", &[]),
        }?
        .l()?;
        let value = value.into();
        let text: String = env.get_string(&value)?.into();
        Ok(text)
    });
    if result.is_err() && env.exception_check().unwrap_or(false) {
        let _ = env.exception_clear();
    }
    result.map_err(|error| error.to_string())
}

/// The Java launcher has extracted this APK's schemas and imported the user's game.
pub fn prepare() -> Result<Vec<String>, String> {
    let app = bevy::android::ANDROID_APP
        .get()
        .ok_or("Android activity is not ready")?;
    let files = app
        .internal_data_path()
        .ok_or("Android internal files directory is unavailable")?;
    // Before the engine starts any threads. Saves/logs and the imported game are separate.
    std::env::set_var("NVFOREVER_HOME", files.join("home"));
    std::env::set_var("NVF_REPORTS", "0");
    std::env::set_var("WGPU_BACKEND", "vulkan");
    std::env::set_var(
        "NVF_PLAY_SCRIPT_FUNCTIONS",
        files.join("runtime/schemas/script/functions.json"),
    );
    crate::android_controller::install();
    let extra: Vec<String> =
        serde_json::from_str(&activity_string("startupArgs", None)?).map_err(|e| e.to_string())?;
    let mut args = crate::android_startup_line(extra);
    for (flag, path) in [
        ("--esm-schema", "runtime/schemas/fnv"),
        ("--nif-schema", "runtime/schemas/nif"),
    ] {
        if !args.iter().any(|s| s.split('=').next() == Some(flag)) {
            args.extend([flag.to_string(), files.join(path).display().to_string()]);
        }
    }
    Ok(args)
}

/// Restart the separate game process through the Android launcher, never an executable.
pub fn restart(args: &[String]) -> Result<(), String> {
    let json = serde_json::to_string(args).map_err(|e| e.to_string())?;
    let result = activity_string("restartEngine", Some(&json))?;
    if result.is_empty() {
        Ok(())
    } else {
        Err(result)
    }
}

/// Return to the launcher and release the game process after the engine exits.
pub fn stopped(reason: &str) {
    if let Err(error) = activity_string("engineStopped", Some(reason)) {
        eprintln!("Android activity exit failed: {error}");
    }
}
