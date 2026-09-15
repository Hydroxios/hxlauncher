use std::{env, path::PathBuf};

fn main() {
    // Cargo executes build scripts with an implementation-defined cwd. Resolve
    // the project .env from CARGO_MANIFEST_DIR so dev and release builds behave
    // identically.
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let env_path = manifest_dir.join("..").join(".env");
    println!("cargo:rerun-if-changed={}", env_path.display());
    let _ = dotenvy::from_path(&env_path);
    for name in ["MICROSOFT_CLIENT_ID", "CURSEFORGE_API_KEY"] {
        println!("cargo:rerun-if-env-changed={name}");
        if let Ok(value) = std::env::var(name) {
            assert!(!value.contains(['\n', '\r']), "Invalid environment value");
            println!("cargo:rustc-env={name}={value}");
        }
    }
    tauri_build::build()
}
