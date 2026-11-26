fn main() {
    // tauri-build embeds icons/icon.ico into the exe but doesn't tell Cargo to
    // watch it, so a new icon was never picked up. Rebuild when icons change.
    println!("cargo:rerun-if-changed=icons");
    tauri_build::build()
}
