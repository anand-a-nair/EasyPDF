//! Tauri build script: generates the context, icons, and platform manifests.

fn main() {
    // Recorded at build time so the sidecar's triple-suffixed name can be
    // matched at runtime. `std::env::consts` cannot reconstruct a full triple.
    let triple = std::env::var("TARGET").unwrap_or_else(|_| "unknown".to_owned());
    println!("cargo:rustc-env=EASYPDF_TARGET_TRIPLE={triple}");

    tauri_build::build();
}
