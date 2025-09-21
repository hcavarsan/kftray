use std::error::Error;
use std::fs::File;

use self_update::{
    backends::github::ReleaseList,
    cargo_crate_version,
};

#[allow(dead_code)]
const REPO_OWNER: &str = "hcavarsan";
#[allow(dead_code)]
const REPO_NAME: &str = "kftray";

#[allow(dead_code)]
fn get_asset_name_for_platform() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "kftui_linux_amd64.tar.gz",
        ("linux", "aarch64") => "kftui_linux_arm64.tar.gz",
        ("macos", _) => "kftui_macos_universal.tar.gz",
        ("windows", "aarch64") => "kftui_windows_arm64.tar.gz",
        ("windows", "x86") => "kftui_windows_x86.tar.gz",
        ("windows", "x86_64") => "kftui_windows_x86_64.tar.gz",
        _ => "kftui_linux_amd64.tar.gz",
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct UpdateInfo {
    pub current_version: String,
    pub latest_version: String,
    pub has_update: bool,
}

#[allow(dead_code)]
pub async fn check_for_updates() -> Result<UpdateInfo, Box<dyn Error>> {
    let current_version = cargo_crate_version!();
    let asset_name = get_asset_name_for_platform();

    let releases = ReleaseList::configure()
        .repo_owner(REPO_OWNER)
        .repo_name(REPO_NAME)
        .build()?
        .fetch()?;

    if let Some(latest_release) = releases.first() {
        let asset_exists = latest_release
            .assets
            .iter()
            .any(|asset| asset.name == asset_name);

        if !asset_exists {
            return Err(format!("Asset {} not found for the latest release", asset_name).into());
        }

        let current_ver = semver::Version::parse(current_version)?;
        let latest_ver = semver::Version::parse(&latest_release.version)?;
        let has_update = latest_ver > current_ver;

        Ok(UpdateInfo {
            current_version: current_version.to_string(),
            latest_version: latest_release.version.clone(),
            has_update,
        })
    } else {
        Err("No releases found".into())
    }
}

#[allow(dead_code)]
pub async fn perform_update() -> Result<String, Box<dyn Error>> {
    let asset_name = get_asset_name_for_platform();

    let releases = ReleaseList::configure()
        .repo_owner(REPO_OWNER)
        .repo_name(REPO_NAME)
        .build()?
        .fetch()?;

    if let Some(latest_release) = releases.first() {
        if let Some(asset) = latest_release
            .assets
            .iter()
            .find(|asset| asset.name == asset_name)
        {
            let tmp_dir = tempfile::tempdir()?;
            let tmp_archive_path = tmp_dir.path().join(&asset.name);
            let mut tmp_file = File::create(&tmp_archive_path)?;

            let version = if latest_release.version.starts_with('v') {
                latest_release.version.clone()
            } else {
                format!("v{}", latest_release.version)
            };
            let direct_url = format!(
                "https://github.com/{}/{}/releases/download/{}/{}",
                REPO_OWNER, REPO_NAME, version, asset.name
            );

            self_update::Download::from_url(&direct_url).download_to(&mut tmp_file)?;

            drop(tmp_file);

            let file_bytes = std::fs::read(&tmp_archive_path)?;
            if file_bytes.len() < 2 || file_bytes[0] != 0x1f || file_bytes[1] != 0x8b {
                return Err(format!(
                    "Downloaded file is not a valid gzip file. First few bytes: {:?}",
                    &file_bytes[..std::cmp::min(20, file_bytes.len())]
                )
                .into());
            }

            let extract_dir = tmp_dir.path().join("extract");
            std::fs::create_dir_all(&extract_dir)?;

            let file = File::open(&tmp_archive_path)?;
            let decoder = flate2::read::GzDecoder::new(file);
            let mut archive = tar::Archive::new(decoder);

            for entry_result in archive.entries()? {
                let mut entry = entry_result?;
                let path = entry.path()?;
                let target_path = extract_dir.join(&path);

                if let Some(parent) = target_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }

                entry.unpack(&target_path)?;
            }

            let archive_name_without_ext =
                asset.name.strip_suffix(".tar.gz").unwrap_or(&asset.name);

            let new_exe = if cfg!(windows) {
                extract_dir.join(format!("{}.exe", archive_name_without_ext))
            } else {
                extract_dir.join(archive_name_without_ext)
            };

            if new_exe.exists() {
                self_update::self_replace::self_replace(new_exe)?;
            } else {
                let entries: Vec<_> = std::fs::read_dir(&extract_dir)?
                    .filter_map(|e| e.ok())
                    .map(|e| e.file_name())
                    .collect();
                return Err(format!(
                    "Could not find {} executable in archive. Found files: {:?}",
                    archive_name_without_ext, entries
                )
                .into());
            }

            Ok(latest_release.version.clone())
        } else {
            Err(format!("Asset {} not found for the latest release", asset_name).into())
        }
    } else {
        Err("No releases found".into())
    }
}
