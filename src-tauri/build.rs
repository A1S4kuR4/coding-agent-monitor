use std::{env, path::Path};

fn main() {
    // The ccusage sidecar is required for any bundle. Fail the Rust build early
    // with an actionable message when the target-triple binary is absent, instead
    // of only discovering the problem at `tauri build` bundle time. Cargo exposes
    // the build target triple to build scripts via the `TARGET` environment var.
    let target_triple = env::var("TARGET").unwrap_or_else(|_| "x86_64-pc-windows-msvc".to_string());
    let sidecar_names = [
        format!("ccusage-{target_triple}.exe"),
        format!("ccusage-antigravity-{target_triple}.exe"),
    ];
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    for sidecar_name in &sidecar_names {
        let sidecar_path = Path::new(&manifest_dir).join("binaries").join(sidecar_name);
        if !sidecar_path.exists() {
            panic!(
                "missing ccusage sidecar for target '{target_triple}': expected {path}. \
                 Stage it by running `pnpm fetch:sidecar`, then rebuild.",
                path = sidecar_path.display()
            );
        }
        println!("cargo:rerun-if-changed=binaries/{sidecar_name}");
    }
    // The `TARGET` triple determines which external binary Tauri packs, so re-run
    // the check when the build target changes.
    println!("cargo:rerun-if-env-changed=TARGET");
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_TRIPLE");

    tauri_build::build()
}
