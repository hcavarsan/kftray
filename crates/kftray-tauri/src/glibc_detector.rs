#[cfg(target_os = "linux")]
use std::process::Command;

#[cfg(target_os = "linux")]
use log::{debug, warn};

#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub struct GlibcVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl GlibcVersion {
    #[allow(dead_code)]
    pub fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    #[allow(dead_code)]
    pub fn is_older_than(&self, other: &GlibcVersion) -> bool {
        if self.major != other.major {
            return self.major < other.major;
        }
        if self.minor != other.minor {
            return self.minor < other.minor;
        }
        self.patch < other.patch
    }
}

#[cfg(target_os = "linux")]
pub fn detect_glibc_version() -> Option<GlibcVersion> {
    debug!("Detecting glibc version on Linux");

    if let Some(version) = detect_via_ldd() {
        debug!(
            "Detected glibc version via ldd: {}.{}.{}",
            version.major, version.minor, version.patch
        );
        return Some(version);
    }

    if let Some(version) = detect_via_gnu_libc_version() {
        debug!(
            "Detected glibc version via gnu_get_libc_version: {}.{}.{}",
            version.major, version.minor, version.patch
        );
        return Some(version);
    }

    if let Some(version) = detect_via_libc_so() {
        debug!(
            "Detected glibc version via libc.so.6: {}.{}.{}",
            version.major, version.minor, version.patch
        );
        return Some(version);
    }

    warn!("Could not detect glibc version, using fallback");
    None
}

#[cfg(not(target_os = "linux"))]
#[allow(dead_code)]
pub fn detect_glibc_version() -> Option<GlibcVersion> {
    None
}

#[cfg(target_os = "linux")]
fn detect_via_ldd() -> Option<GlibcVersion> {
    match Command::new("ldd").arg("--version").output() {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            parse_glibc_version_from_ldd_output(&stdout)
        }
        Err(e) => {
            debug!("Failed to run ldd --version: {}", e);
            None
        }
    }
}

#[cfg(target_os = "linux")]
fn parse_glibc_version_from_ldd_output(output: &str) -> Option<GlibcVersion> {
    for line in output.lines() {
        if line.contains("GLIBC") || line.contains("GNU libc") {
            if let Some(version_part) = line.split_whitespace().last() {
                return parse_version_string(version_part);
            }
        }
    }
    None
}

#[cfg(target_os = "linux")]
fn detect_via_gnu_libc_version() -> Option<GlibcVersion> {
    use std::ffi::CStr;

    unsafe extern "C" {
        fn gnu_get_libc_version() -> *const std::os::raw::c_char;
    }

    unsafe {
        let version_ptr = gnu_get_libc_version();
        if version_ptr.is_null() {
            return None;
        }

        match CStr::from_ptr(version_ptr).to_str() {
            Ok(version_str) => {
                debug!("gnu_get_libc_version returned: {}", version_str);
                parse_version_string(version_str)
            }
            Err(e) => {
                debug!("Failed to convert glibc version to string: {}", e);
                None
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn detect_via_libc_so() -> Option<GlibcVersion> {
    let paths = [
        "/lib/x86_64-linux-gnu/libc.so.6",
        "/lib64/libc.so.6",
        "/lib/libc.so.6",
        "/lib/aarch64-linux-gnu/libc.so.6",
    ];

    for path in &paths {
        if let Some(version) = check_libc_so_version(path) {
            return Some(version);
        }
    }

    None
}

#[cfg(target_os = "linux")]
fn check_libc_so_version(path: &str) -> Option<GlibcVersion> {
    match Command::new("strings").arg(path).output() {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if line.starts_with("GLIBC_") {
                    if let Some(version_part) = line.strip_prefix("GLIBC_") {
                        if let Some(version) = parse_version_string(version_part) {
                            return Some(version);
                        }
                    }
                }
            }
            None
        }
        Err(e) => {
            debug!("Failed to run strings on {}: {}", path, e);
            None
        }
    }
}

#[cfg(target_os = "linux")]
fn parse_version_string(version_str: &str) -> Option<GlibcVersion> {
    debug!("Parsing version string: {}", version_str);

    let cleaned = version_str.trim();
    let parts: Vec<&str> = cleaned.split('.').collect();

    if parts.len() >= 2 {
        if let (Ok(major), Ok(minor)) = (parts[0].parse::<u32>(), parts[1].parse::<u32>()) {
            let patch = if parts.len() >= 3 {
                parts[2].parse::<u32>().unwrap_or(0)
            } else {
                0
            };

            return Some(GlibcVersion::new(major, minor, patch));
        }
    }

    debug!("Failed to parse version string: {}", version_str);
    None
}

#[cfg(target_os = "linux")]
pub fn get_updater_target_platform() -> String {
    if let Some(version) = detect_glibc_version() {
        debug!(
            "Detected glibc version: {}.{}.{}",
            version.major, version.minor, version.patch
        );

        let arch = std::env::consts::ARCH;
        let cutoff_version = GlibcVersion::new(2, 39, 0);

        if version.is_older_than(&cutoff_version) {
            debug!("Using older glibc target");
            format!("linux-{}-glibc231", arch)
        } else {
            debug!("Using newer glibc target");
            format!("linux-{}-glibc239", arch)
        }
    } else {
        warn!("Could not detect glibc version, defaulting to newer glibc target");
        let arch = std::env::consts::ARCH;
        format!("linux-{}-glibc239", arch)
    }
}

#[cfg(not(target_os = "linux"))]
#[allow(dead_code)]
pub fn get_updater_target_platform() -> String {
    "default".to_string()
}

#[cfg(target_os = "linux")]
pub fn get_updater_target_suffix() -> String {
    if let Some(version) = detect_glibc_version() {
        debug!(
            "Detected glibc version: {}.{}.{}",
            version.major, version.minor, version.patch
        );

        let cutoff_version = GlibcVersion::new(2, 39, 0);

        if version.is_older_than(&cutoff_version) {
            debug!("Using older glibc target");
            "-glibc231".to_string()
        } else {
            debug!("Using newer glibc target");
            "-glibc239".to_string()
        }
    } else {
        warn!("Could not detect glibc version, defaulting to newer glibc target");
        "-glibc239".to_string()
    }
}

#[cfg(not(target_os = "linux"))]
#[allow(dead_code)]
pub fn get_updater_target_suffix() -> String {
    "".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_glibc_version_comparison() {
        let older = GlibcVersion::new(2, 31, 0);
        let middle = GlibcVersion::new(2, 36, 0);
        let newer = GlibcVersion::new(2, 39, 0);
        let cutoff = GlibcVersion::new(2, 39, 0);

        assert!(older.is_older_than(&cutoff));
        assert!(middle.is_older_than(&cutoff));
        assert!(!newer.is_older_than(&cutoff));
        assert!(older.is_older_than(&newer));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_version_parsing() {
        assert_eq!(
            parse_version_string("2.31"),
            Some(GlibcVersion::new(2, 31, 0))
        );
        assert_eq!(
            parse_version_string("2.39.1"),
            Some(GlibcVersion::new(2, 39, 1))
        );
        assert_eq!(parse_version_string("invalid"), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_ldd_output_parsing() {
        let output = "ldd (Ubuntu GLIBC 2.31-0ubuntu9.12) 2.31";
        assert_eq!(
            parse_glibc_version_from_ldd_output(output),
            Some(GlibcVersion::new(2, 31, 0))
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_target_suffix() {
        let suffix = get_updater_target_suffix();
        assert!(suffix == "-glibc231" || suffix == "-glibc239");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_target_platform() {
        let platform = get_updater_target_platform();
        let arch = std::env::consts::ARCH;
        assert!(
            platform == format!("linux-{}-glibc231", arch)
                || platform == format!("linux-{}-glibc239", arch)
        );
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn test_non_linux_functions() {
        assert_eq!(get_updater_target_platform(), "default");
        assert_eq!(get_updater_target_suffix(), "");
    }
}
