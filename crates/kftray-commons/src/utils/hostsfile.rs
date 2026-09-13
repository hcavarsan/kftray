use std::{
    fmt,
    fs::OpenOptions,
    io::{
        self,
        BufRead,
        BufReader,
        Write,
    },
    net::IpAddr,
    path::{
        Path,
        PathBuf,
    },
    time::{
        SystemTime,
        UNIX_EPOCH,
    },
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

/// Comment that records which configuration owns a managed line.
const OWNER_MARKER: &str = " # kftray-id=";

/// One line inside a managed section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionEntry {
    pub ip: IpAddr,
    pub hostname: String,
    /// Configuration this entry was written for, when the writer recorded one.
    /// Entries without an owner belong to another writer and are preserved.
    pub owner: Option<String>,
}

/// Serializes hosts-file changes across every process that makes them.
fn with_hosts_lock<T>(work: impl FnOnce() -> Result<T>) -> Result<T> {
    let lock_path = crate::utils::config_dir::get_config_dir()
        .map_err(HostsFileError::InvalidPath)?
        .join("hosts.lock");
    let mut outcome = None;
    crate::utils::config_dir::with_file_lock(&lock_path, || {
        outcome = Some(work());
        Ok(())
    })
    .map_err(HostsFileError::Io)?;

    outcome.unwrap_or_else(|| {
        Err(HostsFileError::Io(
            "Hosts lock produced no result".to_owned(),
        ))
    })
}

pub struct HostsFile {
    entries: Vec<SectionEntry>,
    tag: String,
}

impl HostsFile {
    pub fn new<S: Into<String>>(tag: S) -> Self {
        Self {
            entries: Vec::new(),
            tag: tag.into(),
        }
    }

    pub fn add_entry<S: ToString>(&mut self, ip: IpAddr, hostname: S) -> &mut Self {
        self.entries.push(SectionEntry {
            ip,
            hostname: hostname.to_string(),
            owner: None,
        });
        self
    }

    pub fn add_entries<I, S>(&mut self, ip: IpAddr, hostnames: I) -> &mut Self
    where
        I: IntoIterator<Item = S>,
        S: ToString,
    {
        for hostname in hostnames {
            self.add_entry(ip, hostname);
        }
        self
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// Entries staged for the next write.
    pub fn staged(&self) -> &[SectionEntry] {
        &self.entries
    }

    pub fn write(&self) -> Result<bool> {
        self.write_to(get_default_hosts_path()?)
    }

    /// Whether the hosts file currently carries a section with this tag.
    ///
    /// Lets a caller skip a write it does not need, which matters because the
    /// file is usually only writable with elevated privileges.
    pub fn section_exists(&self) -> Result<bool> {
        let path = get_default_hosts_path()?;
        let contents = std::fs::read_to_string(&path)?;

        Ok(contents.contains(&HostsSection::new(&self.tag).begin_marker()))
    }

    /// Adds an entry that records which configuration owns it.
    ///
    /// The section is shared with the privileged helper, and entries written
    /// without an owner cannot be told apart. The owner is written as a
    /// trailing comment, which the hosts file format ignores, so a writer can
    /// rebuild exactly its own lines and leave every other line alone.
    pub fn add_owned_entry<S: ToString>(
        &mut self, ip: IpAddr, hostname: S, owner: &str,
    ) -> &mut Self {
        self.entries.push(SectionEntry {
            ip,
            hostname: hostname.to_string(),
            owner: Some(owner.to_owned()),
        });
        self
    }

    /// Reads the entries currently inside this tag's section, with the owner
    /// recorded for each.
    pub fn read_section(&self) -> Result<Vec<SectionEntry>> {
        self.read_section_from(get_default_hosts_path()?)
    }

    /// Reads a section from a specific file.
    pub fn read_section_from<P: AsRef<Path>>(&self, path: P) -> Result<Vec<SectionEntry>> {
        let contents = match std::fs::read_to_string(path.as_ref()) {
            Ok(contents) => contents,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        let lines: Vec<String> = contents.lines().map(ToOwned::to_owned).collect();
        let section = HostsSection::new(&self.tag);
        let bounds = section.find_section_bounds(&lines);
        // Half a section is not an empty one: the aliases between a begin
        // marker and a missing end marker still resolve, and reporting nothing
        // would let a caller treat them as already removed.
        if bounds.is_partial() {
            return Err(HostsFileError::InvalidData(format!(
                "Incomplete section markers for tag '{}'",
                self.tag
            )));
        }
        let (Some(begin), Some(end)) = (bounds.begin, bounds.end) else {
            return Ok(Vec::new());
        };
        if end < begin {
            return Err(HostsFileError::InvalidData(format!(
                "Reversed section markers for tag '{}'",
                self.tag
            )));
        }

        let mut entries = Vec::new();
        for line in lines.get(begin + 1..end).unwrap_or_default() {
            // Everything from the first `#` is a comment. Ownership counts
            // only when that comment is the marker itself, so a note that
            // happens to mention the marker cannot make a foreign line look
            // owned, and a real alias is never parsed as comment words.
            let (fields, comment) = match line.split_once('#') {
                Some((fields, comment)) => (fields, Some(comment)),
                None => (line.as_str(), None),
            };
            let owner = comment
                .and_then(|comment| {
                    format!("#{comment}")
                        .strip_prefix(OWNER_MARKER.trim_start())
                        .map(ToOwned::to_owned)
                })
                .map(|owner| owner.trim().to_owned());
            let mut fields = fields.split_whitespace();
            let Some(Ok(ip)) = fields.next().map(str::parse::<IpAddr>) else {
                continue;
            };
            for hostname in fields {
                entries.push(SectionEntry {
                    ip,
                    hostname: hostname.to_owned(),
                    owner: owner.clone(),
                });
            }
        }

        Ok(entries)
    }

    pub fn write_to<P: AsRef<Path>>(&self, path: P) -> Result<bool> {
        let path = path.as_ref();
        validate_hosts_path(path)?;

        // Taken across the whole read-modify-write: the privileged helper is a
        // separate process writing the same file, and rewriting it from a
        // snapshot read outside the lock loses whichever change lost the race.
        with_hosts_lock(|| {
            let writer = HostsFileWriter::new(path);
            writer.update_section(&self.tag, &self.entries)
        })
    }

    /// Reads this tag's section, keeps the entries `keep` accepts, and writes
    /// the result back as one locked operation.
    pub fn retain_section(&self, keep: impl Fn(&SectionEntry) -> bool) -> Result<bool> {
        let path = get_default_hosts_path()?;
        let path = path.as_path();
        validate_hosts_path(path)?;

        with_hosts_lock(|| {
            let kept: Vec<SectionEntry> = self
                .read_section_from(path)?
                .into_iter()
                .filter(&keep)
                .collect();
            let writer = HostsFileWriter::new(path);
            writer.update_section(&self.tag, &kept)
        })
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

    /// One line per entry.
    ///
    /// Grouping several hostnames of one address onto a single line would put
    /// every alias after the first behind the owner comment of that first one,
    /// commenting them out. The SSL aliases alone always share 127.0.0.1.
    fn format_entries(&self, entries: &[SectionEntry]) -> Vec<String> {
        if entries.is_empty() {
            return vec![];
        }

        let mut lines = vec![self.begin_marker()];

        for entry in entries {
            lines.push(match &entry.owner {
                Some(owner) => format!("{} {}{OWNER_MARKER}{owner}", entry.ip, entry.hostname),
                None => format!("{} {}", entry.ip, entry.hostname),
            });
        }

        lines.push(self.end_marker());
        lines
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

    fn update_section(&self, tag: &str, entries: &[SectionEntry]) -> Result<bool> {
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
    fn aliases_sharing_an_address_stay_on_their_own_lines() {
        let (_temp_file, temp_path) = tempfile::NamedTempFile::new().unwrap().into_parts();

        let mut hosts_file = HostsFile::new("test");
        // The SSL aliases of one configuration always share 127.0.0.1.
        hosts_file.add_owned_entry([127, 0, 0, 1].into(), "a.local", "1");
        hosts_file.add_owned_entry([127, 0, 0, 1].into(), "b.local", "2");
        hosts_file.add_entry([127, 0, 0, 1].into(), "plain.local");
        hosts_file.write_to(&temp_path).unwrap();

        let entries = HostsFile::new("test")
            .read_section_from(&temp_path)
            .unwrap();
        assert_eq!(
            entries,
            vec![
                SectionEntry {
                    ip: [127, 0, 0, 1].into(),
                    hostname: "a.local".to_owned(),
                    owner: Some("1".to_owned()),
                },
                SectionEntry {
                    ip: [127, 0, 0, 1].into(),
                    hostname: "b.local".to_owned(),
                    owner: Some("2".to_owned()),
                },
                SectionEntry {
                    ip: [127, 0, 0, 1].into(),
                    hostname: "plain.local".to_owned(),
                    owner: None,
                },
            ]
        );
    }

    #[test]
    fn a_marker_inside_an_ordinary_comment_claims_nothing() {
        let (mut temp_file, temp_path) = tempfile::NamedTempFile::new().unwrap().into_parts();
        temp_file
            .write_all(
                b"# DO NOT EDIT test BEGIN\n\
                  127.0.0.7 real.local # note # kftray-id=42\n\
                  # DO NOT EDIT test END\n",
            )
            .unwrap();

        let entries = HostsFile::new("test")
            .read_section_from(&temp_path)
            .unwrap();

        assert_eq!(
            entries,
            vec![SectionEntry {
                ip: [127, 0, 0, 7].into(),
                hostname: "real.local".to_owned(),
                // A note is not a claim: treating it as one would let a
                // reconciliation delete a mapping it does not own.
                owner: None,
            }]
        );
    }

    #[test]
    fn only_marked_lines_are_claimed_by_their_writer() {
        let (mut temp_file, temp_path) = tempfile::NamedTempFile::new().unwrap().into_parts();
        // A section holding one line from the privileged helper, one from this
        // writer, and one carrying a hand-written comment.
        temp_file
            .write_all(
                b"# DO NOT EDIT test BEGIN\n\
                  127.0.0.5 helper.local\n\
                  127.0.0.6 owned.local # kftray-id=41007\n\
                  127.0.0.7 noted.local # a note\n\
                  # DO NOT EDIT test END\n",
            )
            .unwrap();

        let hosts_file = HostsFile::new("test");
        let entries = hosts_file.read_section_from(&temp_path).unwrap();

        assert_eq!(
            entries,
            vec![
                SectionEntry {
                    ip: [127, 0, 0, 5].into(),
                    hostname: "helper.local".to_owned(),
                    owner: None,
                },
                SectionEntry {
                    ip: [127, 0, 0, 6].into(),
                    hostname: "owned.local".to_owned(),
                    owner: Some("41007".to_owned()),
                },
                // The note is a comment, not two more hostnames: rewriting the
                // line with them would put a real alias after a `#`.
                SectionEntry {
                    ip: [127, 0, 0, 7].into(),
                    hostname: "noted.local".to_owned(),
                    owner: None,
                },
            ]
        );
    }

    #[test]
    fn an_owned_entry_round_trips_through_the_file() {
        let (_temp_file, temp_path) = tempfile::NamedTempFile::new().unwrap().into_parts();

        let mut hosts_file = HostsFile::new("test");
        hosts_file.add_owned_entry([127, 0, 0, 8].into(), "round.local", "9001");
        hosts_file.write_to(&temp_path).unwrap();

        let entries = HostsFile::new("test")
            .read_section_from(&temp_path)
            .unwrap();
        assert_eq!(
            entries,
            vec![SectionEntry {
                ip: [127, 0, 0, 8].into(),
                hostname: "round.local".to_owned(),
                owner: Some("9001".to_owned()),
            }]
        );
    }

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

        // One entry per hostname: aliases of one address need their own lines
        // so an owner comment cannot swallow the ones after it.
        assert_eq!(hosts_file.entries.len(), 3);
    }
}
