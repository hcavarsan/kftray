use std::env;
use std::fs;
use std::path::{
    Path,
    PathBuf,
};
use std::process::Command;

fn main() {
    let output = match Command::new("rustc").args(["-Vv"]).output() {
        Ok(output) => output,
        Err(e) => {
            println!("cargo:warning=Failed to run rustc: {}", e);
            return;
        }
    };

    let output_str = String::from_utf8_lossy(&output.stdout);
    let target_triple = match output_str
        .lines()
        .find(|line| line.starts_with("host:"))
        .and_then(|line| line.split_whitespace().nth(1))
    {
        Some(triple) => triple,
        None => {
            println!("cargo:warning=Failed to determine target triple from rustc output");
            return;
        }
    };

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
        match fs::copy(&helper_bin, &target_path) {
            Ok(_) => {
                println!(
                    "cargo:warning=Helper binary copied to: {}",
                    target_path.display()
                );

                // Ensure the binary is executable on Unix-like systems
                #[cfg(not(target_os = "windows"))]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if let Err(e) =
                        fs::set_permissions(&target_path, fs::Permissions::from_mode(0o755))
                    {
                        println!("cargo:warning=Failed to set executable permissions: {}", e);
                    }
                }
            }
            Err(e) => {
                println!("cargo:warning=Failed to copy helper binary: {}", e);
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
            if let Ok(contents) = fs::read_to_string(&cargo_toml) {
                if contents.contains("[workspace]") {
                    return Some(current_dir);
                }
            }
        }
        if !current_dir.pop() {
            break;
        }
    }
    None
}
