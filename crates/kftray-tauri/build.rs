use std::env;
use std::fs;
use std::path::{
    Path,
    PathBuf,
};

fn main() {
    let target_triple = std::env::var("TARGET").unwrap();

    println!("cargo:warning=Building for target: {target_triple}");

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
    let target_filename = format!("kftray-helper-{target_triple}{extension}");
    let target_path = bin_dir.join(&target_filename);

    println!("cargo:rerun-if-changed={}", target_path.display());

    let workspace_root = find_workspace_root(Path::new(&manifest_dir)).unwrap_or_else(|| {
        println!(
            "cargo:warning=Could not determine workspace root, using manifest dir parent's parent"
        );
        Path::new(&manifest_dir)
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf()
    });

    let helper_bin = workspace_root
        .join("target")
        .join("release")
        .join(format!("kftray-helper{extension}"));

    println!(
        "cargo:warning=Looking for helper binary at: {}",
        helper_bin.display()
    );

    if helper_bin.exists() {
        println!(
            "cargo:warning=Copying helper binary from: {}",
            helper_bin.display()
        );
        match fs::copy(&helper_bin, &target_path) {
            Ok(_) => {
                println!(
                    "cargo:warning=Helper binary copied to: {}",
                    target_path.display()
                );

                #[cfg(not(target_os = "windows"))]
                {
                    println!("cargo:warning=Using default file permissions for binary");
                }
            }
            Err(e) => {
                println!("cargo:warning=Failed to copy helper binary: {e}");
            }
        }
    } else {
        println!(
            "cargo:warning=Helper binary not found at: {}",
            helper_bin.display()
        );
    }

    println!("cargo:rerun-if-changed=build.rs");
    tauri_build::build();
}

fn find_workspace_root(start_dir: &Path) -> Option<PathBuf> {
    let mut current_dir = start_dir.to_path_buf();
    while current_dir.parent().is_some() {
        let cargo_toml = current_dir.join("Cargo.toml");
        if cargo_toml.exists() {
            // Check if it's a workspace by looking for [workspace] section
            if let Ok(contents) = fs::read_to_string(&cargo_toml)
                && contents.contains("[workspace]")
            {
                return Some(current_dir);
            }
        }
        if !current_dir.pop() {
            break;
        }
    }
    None
}
