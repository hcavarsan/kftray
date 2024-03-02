use log::{error, info};
use regex::Regex;
use serde_json::Value;
use std::{
    env, fs, io,
    process::{Command, ExitCode},
};

fn main() -> ExitCode {
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
    let npm_output = Command::new("npm")
        .args(["version", bump_type, "--no-git-tag-version"])
        .output()?;

    if !npm_output.status.success() {
        let error_output = String::from_utf8_lossy(&npm_output.stderr).to_string();
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
        "src-tauri/Cargo.toml",
        new_version,
        update_cargo_toml_version,
    )?;
    log::info!("src-tauri Cargo.toml updated");
    update_file_content(
        "kftray-server/Cargo.toml",
        new_version,
        update_cargo_toml_version,
    )?;
    info!("kftray-server Cargo.toml updated");

    update_file_content("README.md", new_version, update_markdown_version)?;
    log::info!("README.md updated");
    update_file_content(
        "src-tauri/tauri.conf.json",
        new_version,
        update_json_version,
    )?;
    log::info!("tauri.conf.json updated");
    git_tag(new_version)?;
    log::info!("Git tag and push completed");
    log::info!("All versions updated to: {}", new_version);
    Ok(())
}

fn update_file_content<F>(file_path: &str, new_version: &str, update_fn: F) -> io::Result<()>
where
    F: Fn(&str, &str) -> io::Result<String>,
{
    let content = fs::read_to_string(file_path)?;
    let updated_content = update_fn(&content, new_version)?;
    fs::write(file_path, updated_content)?;
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

fn git_tag(new_version: &str) -> io::Result<()> {
    let tag_name = format!("v{}", new_version);
    let git_tag_output = Command::new("git")
        .args(["tag", "-f", &tag_name])
        .output()?;

    if !git_tag_output.status.success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            String::from_utf8_lossy(&git_tag_output.stderr).into_owned(),
        ));
    }
    Ok(())
}
