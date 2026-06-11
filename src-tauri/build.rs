fn main() {
    // Weak-link CoreAudio so the 14.2+ process-tap symbols (e.g.
    // AudioHardwareCreateProcessTap) resolve to NULL on older macOS instead of
    // aborting at dyld load; the runtime gate in shared/audio/system/macos.rs
    // prevents the call there.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg=-Wl,-weak_framework,CoreAudio");
    }

    tauri_build::build()
}
