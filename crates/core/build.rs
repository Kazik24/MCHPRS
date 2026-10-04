use std::{env, fs, path::Path};

fn main() {
    let manifest = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap()).join("../../Cargo.toml");
    println!("cargo:rerun-if-changed={}", manifest.display());
    let content = fs::read_to_string(manifest).unwrap();
    let package = content
        .split("[package]")
        .nth(1)
        .unwrap()
        .split('[')
        .next()
        .unwrap();
    let version = package
        .lines()
        .find_map(|line| {
            let (key, value) = line.split_once('=')?;
            (key.trim() == "version").then(|| value.trim().trim_matches('"'))
        })
        .expect("workspace package version");
    println!("cargo:rustc-env=MCHPRS_VERSION={version}");
}
