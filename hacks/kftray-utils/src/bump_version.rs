use std::{
    env,
    fs,
    io,
    path::Path,
    process::{
        Command,
        ExitCode,
    },
};

use log::{
    error,
    info,
    debug,
};
use regex::Regex;
use serde_json::Value;

fn main() -> ExitCode {
    env_logger::init();

    let args: Vec<String> = env::args().collect();

    const USAGE: &str = "Usage: bump_version <patch|minor|major>";

    const INVALID_ARGUMENT: &str =
        "Invalid argument provided. Expecting 'patch', 'minor' or 'major'.";

    if args.len() != 2 {
        error!(
            "Incorrect number of arguments. {}",
            USAGE.replace("bump_version", &args[0])
        );

        return ExitCode::from(1);
    }

    if !matches!(args[1].as_str(), "patch" | "minor" | "major") {
        error!(
            "{} {}",
            INVALID_ARGUMENT,
            USAGE.replace("bump_version", &args[0])
        );

        return ExitCode::from(1);
    }

    match bump_version(&args[1]) {
        Ok(()) => {
            info!("Version bump completed successfully.");

            ExitCode::SUCCESS
        }
        Err(e) => {
            error!("Version bump failed: {}", e);

            ExitCode::from(1)
        }
    }
}

fn bump_version(bump_type: &str) -> io::Result<()> {
    log::info!("Bumping version to {}", bump_type);

    let dir = "../../frontend";
    let current_dir = env::current_dir()?;
    let absolute_dir = current_dir.join(dir);

    log::info!("Current directory: {:?}", current_dir);
    log::info!("Bumping version to {} in directory {:?}", bump_type, absolute_dir);

    let npm_output = Command::new("npm")
        .args(["version", bump_type, "--no-git-tag-version"])
        .current_dir(&absolute_dir)
        .output()?;

    if !npm_output.status.success() {
        let error_output = String::from_utf8_lossy(&npm_output.stderr).to_string();
        error!("NPM command failed: {}", error_output);

        return Err(io::Error::new(io::ErrorKind::Other, error_output));
    }

    let new_version_tag = String::from_utf8(npm_output.stdout)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?
        .lines()
        .last()
        .unwrap_or_default()
        .trim()
        .to_string();

    let new_version = new_version_tag
        .strip_prefix('v')
        .unwrap_or(&new_version_tag);

    log::info!("NPM version successfully bumped to: {}", new_version);

    info!("NPM version successfully bumped to: {}", new_version);

    info!("Updating version in Cargo.toml, README.md and tauri.conf.json");

    update_file_content(
        "../../crates/kftray-tauri/Cargo.toml",
        new_version,
        update_cargo_toml_version,
    )?;

    log::info!("kftray-tauri Cargo.toml updated");

    update_file_content(
        "../../crates/kftray-server/Cargo.toml",
        new_version,
        update_cargo_toml_version,
    )?;

    log::info!("kftray-server Cargo.toml updated");

	update_file_content(
        "../../crates/kftui/Cargo.toml",
        new_version,
        update_cargo_toml_version,
    )?;

    log::info!("kftui Cargo.toml updated");

    update_file_content("../../README.md", new_version, update_markdown_version)?;

    log::info!("README.md updated");

    update_file_content(
        "../../crates/kftray-tauri/tauri.conf.json",
        new_version,
        update_json_version,
    )?;

    log::info!("tauri.conf.json updated");


    log::info!("All versions updated to: {}", new_version);

    Ok(())
}

fn update_file_content<F>(file_path: &str, new_version: &str, update_fn: F) -> io::Result<()>
where
    F: Fn(&str, &str) -> io::Result<String>,
{
    debug!("Reading file: {}", file_path);
    let content = fs::read_to_string(file_path).map_err(|e| {
        io::Error::new(
            io::ErrorKind::Other,
            format!("Failed to read {}: {}", file_path, e),
        )
    })?;

    debug!("Updating content for file: {}", file_path);
    let updated_content = update_fn(&content, new_version).map_err(|e| {
        io::Error::new(
            io::ErrorKind::Other,
            format!("Failed to update content for {}: {}", file_path, e),
        )
    })?;

    debug!("Writing updated content to file: {}", file_path);
    fs::write(file_path, updated_content).map_err(|e| {
        io::Error::new(
            io::ErrorKind::Other,
            format!("Failed to write updated content to {}: {}", file_path, e),
        )
    })?;

    Ok(())
}

fn update_cargo_toml_version(content: &str, new_version: &str) -> io::Result<String> {
    let package_section_regex = Regex::new(r"^\[package\]\s*$")
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    let version_regex = Regex::new(r#"version = "\d+\.\d+\.\d+""#)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    let mut in_package_section = false;

    let mut updated_lines = Vec::new();

    for line in content.lines() {
        if package_section_regex.is_match(line) {
            in_package_section = true;
        } else if line.starts_with('[') {
            in_package_section = false;
        }

        if in_package_section && version_regex.is_match(line) {
            updated_lines.push(format!(r#"version = "{}""#, new_version));

            in_package_section = false;
        } else {
            updated_lines.push(line.to_string());
        }
    }

    Ok(updated_lines.join("\n"))
}

fn update_markdown_version(content: &str, new_version: &str) -> io::Result<String> {
    let version_regex = Regex::new(r"kftray_\d+\.\d+\.\d+")
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    Ok(version_regex
        .replace_all(content, format!("kftray_{}", new_version))
        .into_owned())
}

fn update_json_version(content: &str, new_version: &str) -> io::Result<String> {
    let mut json_content: Value =
        serde_json::from_str(content).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    if let Some(package) = json_content.get_mut("package") {
        package["version"] = serde_json::Value::String(new_version.to_string());
    } else {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "Missing 'package' section in tauri.conf.json",
        ));
    }

    serde_json::to_string_pretty(&json_content)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}
