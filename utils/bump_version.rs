use log::{error, info};
use regex::Regex;
use serde_json::{json, Value};
use std::{
    env, fs, io,
    process::{Command, ExitCode},
};

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    const USAGE: &str = "Usage: bump_version [patch|minor|major]";

    if args.len() != 2 {
        error!("{}", USAGE.replace("bump_version", &args[0]));
        return ExitCode::from(1);
    }

    match bump_version(&args[1]) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            error!("Error: {:?}", e);
            ExitCode::from(1)
        }
    }
}

fn bump_version(bump_type: &str) -> io::Result<()> {
    let npm_output = Command::new("npm")
        .args(["version", bump_type, "--no-git-tag-version"])
        .output()?;

    if !npm_output.status.success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            String::from_utf8_lossy(&npm_output.stderr),
        ));
    }

    let output_str = String::from_utf8_lossy(&npm_output.stdout);
    let new_version_tag = output_str.lines().last().unwrap_or_default().trim();
    let new_version = new_version_tag.strip_prefix('v').unwrap_or(new_version_tag);

    info!("NPM version successfully bumped to {}", new_version);

    update_cargo_toml_version("src-tauri/Cargo.toml", new_version)?;
    update_readme_md_version("README.md", new_version)?;
    update_tauri_conf_version("src-tauri/tauri.conf.json", new_version)?;
    git_tag(new_version)?;

    info!("Version updated in Cargo.toml and README.md");
    println!("Version bumped to {}", new_version);
    Ok(())
}

fn update_cargo_toml_version(cargo_toml_path: &str, new_version: &str) -> io::Result<()> {
    let data = fs::read_to_string(cargo_toml_path)?;
    let mut lines: Vec<_> = data.lines().map(String::from).collect();

    let package_section_regex = Regex::new(r"^\[package\]\s*$")
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    let version_regex = Regex::new(r#"^version = "\d+\.\d+\.\d+""#)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    let mut in_package_section = false;

    for line in &mut lines {
        if package_section_regex.is_match(line) {
            in_package_section = true;
        } else if line.starts_with('[') && line.ends_with(']') {
            in_package_section = false;
        }

        if in_package_section && version_regex.is_match(line) {
            *line = format!(r#"version = "{}""#, new_version);
            break;
        }
    }

    fs::write(cargo_toml_path, lines.join("\n").as_bytes())?;

    Ok(())
}

fn update_readme_md_version(readme_md_path: &str, new_version: &str) -> io::Result<()> {
    let data = fs::read_to_string(readme_md_path)?;
    let version_regex = Regex::new(r"kftray_\d+\.\d+\.\d+")
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    let updated_data = version_regex
        .replace_all(&data, format!("kftray_{}", new_version))
        .to_string();

    fs::write(readme_md_path, updated_data.as_bytes())?;
    Ok(())
}

fn update_tauri_conf_version(tauri_conf_path: &str, new_version: &str) -> io::Result<()> {
    let content = fs::read_to_string(tauri_conf_path)?;

    let mut json_content: Value = serde_json::from_str(&content)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    if let Some(package) = json_content.get_mut("package") {
        package["version"] = json!(new_version);
    } else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Missing 'package' section in tauri.conf.json",
        ));
    }

    fs::write(
        tauri_conf_path,
        serde_json::to_string_pretty(&json_content)?.as_bytes(),
    )?;

    Ok(())
}

fn git_tag(new_version: &str) -> std::io::Result<()> {
    let git_output = Command::new("git")
        .args(["tag", "-f", &format!("v{}", new_version)])
        .output()?;

    if !git_output.status.success() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            String::from_utf8_lossy(&git_output.stderr),
        ));
    }

    Command::new("git")
        .args(["push", "origin", &format!("v{}", new_version), "--force"])
        .output()?;

    Ok(())
}
