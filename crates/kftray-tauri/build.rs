use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;

fn main() {
    let output = Command::new("rustc")
        .args(["-Vv"])
        .output()
        .expect("Failed to run rustc");

    let output_str = String::from_utf8_lossy(&output.stdout);
    let target_triple = output_str
        .lines()
        .find(|line| line.starts_with("host:"))
        .and_then(|line| line.split_whitespace().nth(1))
        .expect("Failed to determine target triple");

    println!("cargo:warning=Building for target: {}", target_triple);

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let bin_dir = Path::new(&manifest_dir).join("bin");

    if !bin_dir.exists() {
        fs::create_dir_all(&bin_dir).expect("Failed to create bin directory");
    }

    let extension = if cfg!(target_os = "windows") {
        ".exe"
    } else {
        ""
    };
    let target_filename = format!("kftray-helper-{}{}", target_triple, extension);
    let target_path = bin_dir.join(&target_filename);

    println!("cargo:rerun-if-changed={}", target_path.display());

    let workspace_root = Path::new(&manifest_dir).parent().unwrap().parent().unwrap();

    let helper_bin = workspace_root
        .join("target")
        .join("release")
        .join(format!("kftray-helper{}", extension));

    println!(
        "cargo:warning=Looking for helper binary at: {}",
        helper_bin.display()
    );

    if helper_bin.exists() {
        println!(
            "cargo:warning=Copying helper binary from: {}",
            helper_bin.display()
        );
        fs::copy(&helper_bin, &target_path).expect("Failed to copy helper binary");
        println!(
            "cargo:warning=Helper binary copied to: {}",
            target_path.display()
        );
    } else {
        println!(
            "cargo:warning=Helper binary not found at: {}",
            helper_bin.display()
        );
    }

    println!("cargo:rerun-if-changed=build.rs");
    tauri_build::build();
}
