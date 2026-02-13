use std::{
    collections::BTreeMap,
    fmt,
    fs::OpenOptions,
    io::{self, BufRead, BufReader, Write},
    net::IpAddr,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub type Result<T> = std::result::Result<T, HostsFileError>;

#[derive(Debug, Clone)]
pub enum HostsFileError {
    Io(String),
    InvalidPath(String),
    InvalidData(String),
    UnsupportedPlatform,
}

impl fmt::Display for HostsFileError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Io(msg) => write!(f, "IO error: {}", msg),
            Self::InvalidPath(msg) => write!(f, "Invalid path: {}", msg),
            Self::InvalidData(msg) => write!(f, "Invalid data: {}", msg),
            Self::UnsupportedPlatform => write!(f, "Unsupported platform"),
        }
    }
}

impl std::error::Error for HostsFileError {}

impl From<io::Error> for HostsFileError {
    fn from(err: io::Error) -> Self {
        Self::Io(err.to_string())
    }
}

pub struct HostsFile {
    entries: BTreeMap<IpAddr, Vec<String>>,
    tag: String,
}

impl HostsFile {
    pub fn new<S: Into<String>>(tag: S) -> Self {
        Self {
            entries: BTreeMap::new(),
            tag: tag.into(),
        }
    }

    pub fn add_entry<S: ToString>(&mut self, ip: IpAddr, hostname: S) -> &mut Self {
        self.entries
            .entry(ip)
            .or_default()
            .push(hostname.to_string());
        self
    }

    pub fn add_entries<I, S>(&mut self, ip: IpAddr, hostnames: I) -> &mut Self
    where
        I: IntoIterator<Item = S>,
        S: ToString,
    {
        self.entries
            .entry(ip)
            .or_default()
            .extend(hostnames.into_iter().map(|h| h.to_string()));
        self
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    pub fn write(&self) -> Result<bool> {
        self.write_to(get_default_hosts_path()?)
    }

    pub fn write_to<P: AsRef<Path>>(&self, path: P) -> Result<bool> {
        let path = path.as_ref();
        validate_hosts_path(path)?;

        let writer = HostsFileWriter::new(path);
        writer.update_section(&self.tag, &self.entries)
    }
}

struct HostsSection {
    tag: String,
}

impl HostsSection {
    fn new(tag: &str) -> Self {
        Self {
            tag: tag.to_string(),
        }
    }

    fn begin_marker(&self) -> String {
        format!("# DO NOT EDIT {} BEGIN", self.tag)
    }

    fn end_marker(&self) -> String {
        format!("# DO NOT EDIT {} END", self.tag)
    }

    fn format_entries(&self, entries: &BTreeMap<IpAddr, Vec<String>>) -> Vec<String> {
        if entries.is_empty() {
            return vec![];
        }

        let mut lines = vec![self.begin_marker()];

        for (ip, hostnames) in entries {
            lines.extend(self.format_host_entries(ip, hostnames));
        }

        lines.push(self.end_marker());
        lines
    }

    fn format_host_entries(&self, ip: &IpAddr, hostnames: &[String]) -> Vec<String> {
        if cfg!(windows) {
            hostnames
                .iter()
                .map(|hostname| format!("{} {}", ip, hostname))
                .collect()
        } else {
            vec![format!("{} {}", ip, hostnames.join(" "))]
        }
    }

    fn find_section_bounds(&self, lines: &[String]) -> SectionBounds {
        let begin_marker = self.begin_marker();
        let end_marker = self.end_marker();

        let begin = lines.iter().position(|line| line.trim() == begin_marker);
        let end = lines.iter().position(|line| line.trim() == end_marker);

        SectionBounds { begin, end }
    }
}

#[derive(Debug)]
struct SectionBounds {
    begin: Option<usize>,
    end: Option<usize>,
}

impl SectionBounds {
    fn is_complete(&self) -> bool {
        self.begin.is_some() && self.end.is_some()
    }

    fn is_missing(&self) -> bool {
        self.begin.is_none() && self.end.is_none()
    }

    fn is_partial(&self) -> bool {
        !self.is_complete() && !self.is_missing()
    }
}

struct HostsFileWriter<'a> {
    path: &'a Path,
}

impl<'a> HostsFileWriter<'a> {
    fn new(path: &'a Path) -> Self {
        Self { path }
    }

    fn update_section(&self, tag: &str, entries: &BTreeMap<IpAddr, Vec<String>>) -> Result<bool> {
        let mut lines = self.read_file_lines()?;
        let section = HostsSection::new(tag);
        let new_section_lines = section.format_entries(entries);

        let changed = self.apply_section_update(&mut lines, &section, new_section_lines)?;

        if changed {
            self.write_file_lines(&lines)?;
        }

        Ok(changed)
    }

    fn read_file_lines(&self) -> Result<Vec<String>> {
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(self.path)?;

        Ok(BufReader::new(file)
            .lines()
            .collect::<io::Result<Vec<_>>>()?)
    }

    fn apply_section_update(
        &self, lines: &mut Vec<String>, section: &HostsSection, new_section_lines: Vec<String>,
    ) -> Result<bool> {
        let bounds = section.find_section_bounds(lines);

        if bounds.is_partial() {
            return Err(HostsFileError::InvalidData(format!(
                "Incomplete section markers for tag '{}'",
                section.tag
            )));
        }

        if bounds.is_complete() {
            self.replace_existing_section(lines, &bounds, new_section_lines)
        } else {
            self.add_new_section(lines, new_section_lines)
        }
    }

    fn replace_existing_section(
        &self, lines: &mut Vec<String>, bounds: &SectionBounds, new_section_lines: Vec<String>,
    ) -> Result<bool> {
        let begin = bounds.begin.unwrap();
        let end = bounds.end.unwrap();

        let old_section: Vec<String> = lines.drain(begin..=end).collect();

        if old_section == new_section_lines {
            lines.splice(begin..begin, old_section);
            return Ok(false);
        }

        lines.splice(begin..begin, new_section_lines);
        Ok(true)
    }

    fn add_new_section(
        &self, lines: &mut Vec<String>, new_section_lines: Vec<String>,
    ) -> Result<bool> {
        if new_section_lines.is_empty() {
            return Ok(false);
        }

        if let Some(last_line) = lines.last()
            && !last_line.is_empty()
        {
            lines.push(String::new());
        }

        lines.extend(new_section_lines);
        Ok(true)
    }

    fn write_file_lines(&self, lines: &[String]) -> Result<()> {
        let content = self.format_file_content(lines)?;
        let writer = AtomicFileWriter::new(self.path);
        writer.write_content(&content)
    }

    fn format_file_content(&self, lines: &[String]) -> Result<Vec<u8>> {
        let mut buffer = Vec::new();
        for line in lines {
            writeln!(buffer, "{}", line)?;
        }
        Ok(buffer)
    }
}

struct AtomicFileWriter<'a> {
    target_path: &'a Path,
}

impl<'a> AtomicFileWriter<'a> {
    fn new(path: &'a Path) -> Self {
        Self { target_path: path }
    }

    fn write_content(&self, content: &[u8]) -> Result<()> {
        match self.try_atomic_write(content) {
            Ok(()) => {
                log::debug!("Successfully wrote hosts file using atomic write");
                Ok(())
            }
            Err(_) => {
                log::debug!("Atomic write failed, falling back to direct write");
                self.write_directly(content)
            }
        }
    }

    fn try_atomic_write(&self, content: &[u8]) -> Result<()> {
        let temp_path = self.create_temp_path()?;

        std::fs::copy(self.target_path, &temp_path)?;

        #[cfg(target_os = "linux")]
        self.preserve_selinux_context(&temp_path);

        self.write_file(&temp_path, content)?;
        std::fs::rename(&temp_path, self.target_path)?;

        Ok(())
    }

    fn create_temp_path(&self) -> Result<PathBuf> {
        let parent = self.target_path.parent().ok_or_else(|| {
            HostsFileError::InvalidPath("Path has no parent directory".to_string())
        })?;

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("System time is before Unix epoch")
            .as_millis();

        let filename = self
            .target_path
            .file_name()
            .ok_or_else(|| HostsFileError::InvalidPath("Path has no filename".to_string()))?;

        let temp_filename = format!("{}.tmp{}", filename.to_string_lossy(), timestamp);
        Ok(parent.join(temp_filename))
    }

    #[cfg(target_os = "linux")]
    fn preserve_selinux_context(&self, _temp_path: &Path) {
        log::trace!("SELinux context preservation not implemented");
    }

    fn write_directly(&self, content: &[u8]) -> Result<()> {
        self.write_file(self.target_path, content)
    }

    fn write_file(&self, path: &Path, content: &[u8]) -> Result<()> {
        OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(path)?
            .write_all(content)?;
        Ok(())
    }
}

fn get_default_hosts_path() -> Result<PathBuf> {
    let path = get_platform_hosts_path()?;

    if !path.exists() {
        return Err(HostsFileError::InvalidPath(format!(
            "Hosts file not found at {}",
            path.display()
        )));
    }

    Ok(path)
}

fn get_platform_hosts_path() -> Result<PathBuf> {
    if cfg!(unix) {
        Ok(PathBuf::from("/etc/hosts"))
    } else if cfg!(windows) {
        let windir = std::env::var("WinDir").map_err(|_| {
            HostsFileError::InvalidPath("WinDir environment variable not found".to_string())
        })?;
        Ok(PathBuf::from(format!(
            "{}\\System32\\Drivers\\Etc\\hosts",
            windir
        )))
    } else {
        Err(HostsFileError::UnsupportedPlatform)
    }
}

fn validate_hosts_path(path: &Path) -> Result<()> {
    if path.is_dir() {
        Err(HostsFileError::InvalidPath(
            "Expected file path, got directory".to_string(),
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    #[test]
    fn test_hosts_file_write() {
        let (mut temp_file, temp_path) = tempfile::NamedTempFile::new().unwrap().into_parts();
        temp_file.write_all(b"preexisting\ncontent").unwrap();

        let mut hosts_file = HostsFile::new("test");
        hosts_file.add_entry([1, 1, 1, 1].into(), "example.com");

        assert!(hosts_file.write_to(&temp_path).unwrap());
        assert!(!hosts_file.write_to(&temp_path).unwrap());

        let contents = std::fs::read_to_string(&temp_path).unwrap();
        assert!(contents.contains("preexisting\ncontent"));
        assert!(contents.contains("# DO NOT EDIT test BEGIN"));
        assert!(contents.contains("1.1.1.1 example.com"));
        assert!(contents.contains("# DO NOT EDIT test END"));
    }

    #[test]
    fn test_fluent_api() {
        let mut hosts_file = HostsFile::new("test");
        hosts_file
            .add_entry([127, 0, 0, 1].into(), "localhost")
            .add_entries([192, 168, 1, 1].into(), ["router", "gateway"]);

        assert_eq!(hosts_file.entries.len(), 2);
    }
}
