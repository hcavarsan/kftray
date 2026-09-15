use std::{
    collections::HashSet,
    fmt,
    fs::OpenOptions,
    io::{
        self,
        Write,
    },
    net::IpAddr,
    path::{
        Path,
        PathBuf,
    },
};

pub type Result<T, E = HostsFileError> = std::result::Result<T, E>;

#[derive(Debug, Clone)]
pub enum HostsFileError {
    Io(String),
    /// The caller may not write the file. Kept apart from other I/O errors
    /// because a caller with a privileged writer at hand can route the same
    /// edit through it.
    PermissionDenied(String),
    InvalidPath(String),
    InvalidData(String),
    UnsupportedPlatform,
}

impl fmt::Display for HostsFileError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Io(msg) => write!(f, "IO error: {}", msg),
            Self::PermissionDenied(msg) => write!(f, "Permission denied: {}", msg),
            Self::InvalidPath(msg) => write!(f, "Invalid path: {}", msg),
            Self::InvalidData(msg) => write!(f, "Invalid data: {}", msg),
            Self::UnsupportedPlatform => write!(f, "Unsupported platform"),
        }
    }
}

impl std::error::Error for HostsFileError {}

impl From<io::Error> for HostsFileError {
    fn from(err: io::Error) -> Self {
        if err.kind() == io::ErrorKind::PermissionDenied {
            Self::PermissionDenied(err.to_string())
        } else {
            Self::Io(err.to_string())
        }
    }
}

impl From<HostsFileError> for io::Error {
    fn from(err: HostsFileError) -> Self {
        match err {
            HostsFileError::PermissionDenied(msg) => {
                io::Error::new(io::ErrorKind::PermissionDenied, msg)
            }
            other => io::Error::other(other),
        }
    }
}

/// Trailing comment that records which configuration a line belongs to.
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

/// Rejects an owner that could change the structure of the file.
///
/// Ids are plain tokens (`42`, `42-https-local`), so anything outside that
/// alphabet is a mistake or an injection, never a real owner.
pub fn validate_owner(owner: &str) -> Result<()> {
    if owner.is_empty()
        || !owner
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':'))
    {
        return Err(HostsFileError::InvalidData(format!(
            "Invalid hosts entry owner {owner:?}"
        )));
    }
    Ok(())
}

/// Rejects a hostname that would not stay on its own line.
pub fn validate_hostname(hostname: &str) -> Result<()> {
    if hostname.is_empty() || hostname.chars().any(|c| c.is_whitespace() || c == '#') {
        return Err(HostsFileError::InvalidData(format!(
            "Invalid hostname {hostname:?}"
        )));
    }
    Ok(())
}

/// Runs `work` while holding an exclusive lock on the hosts file itself.
///
/// The file is the resource, so it is also the lock: there is no separate
/// path to agree on between the unprivileged application and the privileged
/// helper, nothing to create, nothing to chmod and nothing a symlink could
/// redirect. It is opened read-only, which every writer can do.
///
/// On Unix the writer replaces the file by renaming a temporary over it. The
/// lock lives on the inode, so a process that acquires it after such a rename
/// holds a lock on the file that was just retired and no longer excludes
/// anyone. That case is detected by comparing the locked descriptor with the
/// path and starting over, which makes the lock on the current inode
/// exclusive for as long as its holder is the one doing the rename.
///
/// Windows locks are mandatory and would block the holder's own write, so the
/// lock covers one byte far past the end of the file instead of its content.
fn with_hosts_lock<T>(path: &Path, recover: bool, work: impl FnOnce() -> Result<T>) -> Result<T> {
    use crate::utils::config_dir::{
        LockRegion,
        unlock,
    };

    /// Releases the lock when dropped, including on unwind, so a panic in
    /// `work` cannot leave the lock held until the file descriptor closes.
    struct UnlockOnDrop<'a>(&'a std::fs::File);

    impl Drop for UnlockOnDrop<'_> {
        fn drop(&mut self) {
            unlock(self.0, LockRegion::PendingByte);
        }
    }

    match open_locked(path, recover)? {
        Some(file) => {
            let _unlock = UnlockOnDrop(&file);
            work()
        }
        None => work(),
    }
}

/// Opens the hosts file read-only and takes its lock.
///
/// On Unix the lock is retaken until the locked descriptor and the path name
/// the same inode, which is what excludes a holder whose lock is on a file a
/// rename just retired.
/// Opens the hosts file read-only, creating it first when `create` is set
/// and a custom path names a file that does not exist yet.
///
/// The system file is never created here: a missing one is an error the
/// caller reports before reaching this. A custom path, as the legacy
/// per-configuration cleanup and tests use, starts empty. `create(true)`
/// tolerates another process winning the creation, so two writers starting
/// on the same fresh path both end up locking the one file. A read passes
/// `create: false` and promises never to create the file: a path that does
/// not exist yields `Ok(None)` instead of one.
fn open_for_lock(path: &Path, create: bool) -> Result<Option<std::fs::File>> {
    match OpenOptions::new().read(true).open(path) {
        Ok(file) => Ok(Some(file)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if !create {
                return Ok(None);
            }
            Ok(Some(
                OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create(true)
                    .truncate(false)
                    .open(path)?,
            ))
        }
        Err(error) => Err(error.into()),
    }
}

/// Retries `attempt` until it reports success, sleeping `delay` between
/// failures, up to `max_attempts` times.
///
/// A rename racing the lock can keep losing indefinitely; without a bound
/// that turns into a hang instead of a reported error.
#[cfg(unix)]
fn retry_bounded<T>(
    what: &str, max_attempts: u32, delay: std::time::Duration,
    mut attempt: impl FnMut() -> Result<Option<T>>,
) -> Result<T> {
    for remaining in (0..max_attempts).rev() {
        if let Some(value) = attempt()? {
            return Ok(value);
        }
        if remaining > 0 {
            std::thread::sleep(delay);
        }
    }
    Err(HostsFileError::Io(format!(
        "Timed out waiting for a stable lock on {what} after {max_attempts} attempts"
    )))
}

/// Opens the hosts file's lock, or reports that there is nothing to lock.
///
/// `recover` doubles as "this is a write": only a write may create a
/// missing file, so a read that finds nothing yields `Ok(None)` instead of
/// creating one and locking it.
#[cfg(unix)]
fn open_locked(path: &Path, recover: bool) -> Result<Option<std::fs::File>> {
    use std::os::unix::fs::MetadataExt;

    use crate::utils::config_dir::{
        LockRegion,
        unlock,
        wait_for_exclusive_lock,
    };

    let what = path.display().to_string();
    retry_bounded(&what, 50, std::time::Duration::from_millis(20), || {
        let Some(file) = open_for_lock(path, recover)? else {
            return Ok(Some(None));
        };
        wait_for_exclusive_lock(&file, LockRegion::PendingByte, &what)
            .map_err(HostsFileError::Io)?;

        let locked = file.metadata()?;
        match std::fs::metadata(path) {
            Ok(current) if current.dev() == locked.dev() && current.ino() == locked.ino() => {
                Ok(Some(Some(file)))
            }
            Ok(_) => {
                unlock(&file, LockRegion::PendingByte);
                Ok(None)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound && !recover => {
                unlock(&file, LockRegion::PendingByte);
                Ok(Some(None))
            }
            Err(error) => {
                unlock(&file, LockRegion::PendingByte);
                Err(error.into())
            }
        }
    })
}

/// Where the content of an in-place rewrite is published before it is
/// applied, and where a read looks for one that never completed.
///
/// The rewrite truncates the hosts file, so the new content is committed to
/// this file first: a rewrite interrupted at any point is completed from it
/// the next time the file is opened, rather than leaving the file truncated.
/// It exists only while a rewrite is outstanding; an incomplete one is never
/// visible because it is written to a temporary name and renamed into place.
///
/// A sibling of the hosts file itself is the only location: it is writable
/// by exactly the principal that can write the hosts file (typically
/// `drivers\etc\`, which requires the same elevation hosts editing does), so
/// planting one needs no less privilege than editing hosts directly. A
/// directory outside the hosts tree, such as `%ProgramData%`, is writable by
/// far less privileged callers, which would let an unprivileged process
/// stage a pending rewrite for an elevated recovery to apply over the real
/// file.
#[cfg(windows)]
fn pending_path(path: &Path) -> PathBuf {
    let mut pending = path.as_os_str().to_owned();
    pending.push(".kftray-pending");
    PathBuf::from(pending)
}

/// Confirms that `path`, when it lexically names the platform hosts file,
/// still resolves to it once reparse points in its ancestry are followed.
///
/// Recovery below trusts `path` completely: it applies a pending rewrite to
/// whatever it opens without re-deriving the location. A directory in the
/// hosts file's ancestry replaced by a junction would not show up on
/// `validate_hosts_path`'s check of the file itself, so it would otherwise
/// survive to make recovery apply a planted pending copy to whatever the
/// junction actually points at. A custom path, which never equals the
/// platform path lexically, has no fixed location to compare against and is
/// left alone.
#[cfg(windows)]
fn verify_platform_hosts_path(path: &Path) -> Result<()> {
    let platform = get_platform_hosts_path()?;
    if platform != path {
        return Ok(());
    }
    if path.canonicalize()? != platform.canonicalize()? {
        return Err(HostsFileError::InvalidPath(
            "Hosts path does not resolve to the platform hosts file".to_string(),
        ));
    }
    Ok(())
}

/// Opens the hosts file read-only and takes its lock, or reports that there
/// is nothing to lock.
///
/// The writer rewrites the file in place on Windows, so the locked handle
/// stays the current file and no identity check is needed there. A rewrite
/// that did not complete is completed here, under the lock, when `recover`
/// is set: the edit path repairs it before writing again, while a read
/// leaves it in place, never creates the file, and reads the pending copy
/// directly instead.
#[cfg(windows)]
fn open_locked(path: &Path, recover: bool) -> Result<Option<std::fs::File>> {
    use crate::utils::config_dir::{
        LockRegion,
        wait_for_exclusive_lock,
    };

    if recover {
        verify_platform_hosts_path(path)?;
    }
    let Some(file) = open_for_lock(path, recover)? else {
        return Ok(None);
    };
    wait_for_exclusive_lock(&file, LockRegion::PendingByte, &path.display().to_string())
        .map_err(HostsFileError::Io)?;
    if recover {
        let pending = pending_path(path);
        validate_hosts_path(&pending)?;
        if pending.exists() {
            log::warn!(
                "Completing an interrupted rewrite of the hosts file from {}",
                pending.display()
            );
            std::fs::copy(&pending, path)?;
            // Durable before the copy it was restored from goes: a power loss
            // after the removal would otherwise leave the file partial with
            // nothing left to complete it from.
            OpenOptions::new().write(true).open(path)?.sync_all()?;
            if let Err(error) = std::fs::remove_file(&pending) {
                log::warn!(
                    "Could not remove stale pending hosts rewrite at {}: {error}",
                    pending.display()
                );
            }
        }
    }
    Ok(Some(file))
}

/// One physical line, with the terminator it had on disk.
///
/// A line kftray did not touch is written back with exactly the terminator
/// it was read with; only a line kftray itself formats picks the file's
/// dominant terminator. Without this, rewriting one line of a Windows
/// hosts file through `str::lines()` and `writeln!` silently turned every
/// untouched CRLF line into LF.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Line {
    text: String,
    crlf: bool,
}

impl Line {
    fn new(text: impl Into<String>, crlf: bool) -> Self {
        Self {
            text: text.into(),
            crlf,
        }
    }

    fn is_empty(&self) -> bool {
        self.text.is_empty()
    }
}

/// The hosts file as raw lines, edited under one lock and written once.
///
/// Every operation preserves the lines it was not asked to touch exactly as
/// they were: a comment, a blank line or an alias written by hand or by another
/// writer is never reformatted or dropped, and a document that ends up
/// unchanged is never written. That last property is what lets an
/// unprivileged caller find out that nothing of its own is on disk without
/// needing permission to write.
pub struct HostsDocument {
    lines: Vec<Line>,
    original: Vec<Line>,
    /// Whether the file, as read, ended in a newline after its last line.
    ends_with_newline: bool,
}

/// One parsed line of a managed section.
struct ParsedLine {
    ip: IpAddr,
    hostnames: Vec<String>,
    owner: Option<String>,
}

impl HostsDocument {
    fn load(path: &Path) -> Result<Self> {
        let contents = Self::read_intended_content(path)?;
        let (lines, ends_with_newline) = Self::split_content(&contents);
        Ok(Self {
            original: lines.clone(),
            lines,
            ends_with_newline,
        })
    }

    /// Splits raw file content into lines, keeping each line's own CRLF/LF
    /// terminator, and whether the content ended in a newline at all.
    ///
    /// `str::lines()` cannot be used here: it discards the terminator, which
    /// is exactly what an untouched line must keep across a rewrite.
    fn split_content(contents: &str) -> (Vec<Line>, bool) {
        if contents.is_empty() {
            return (Vec::new(), true);
        }
        let ends_with_newline = contents.ends_with('\n');
        let mut raw: Vec<&str> = contents.split('\n').collect();
        if ends_with_newline {
            // The split on the final `\n` leaves an empty trailing element
            // that is not a line.
            raw.pop();
        }
        let lines = raw
            .into_iter()
            .map(|line| match line.strip_suffix('\r') {
                Some(stripped) => Line::new(stripped, true),
                None => Line::new(line, false),
            })
            .collect();
        (lines, ends_with_newline)
    }

    #[cfg(not(windows))]
    fn read_intended_content(path: &Path) -> Result<String> {
        match std::fs::read_to_string(path) {
            Ok(contents) => Ok(contents),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(String::new()),
            Err(error) => Err(error.into()),
        }
    }

    /// The content a read should see: a pending rewrite for this exact path,
    /// when one exists, or the file itself.
    ///
    /// A pending copy is the intended state of a write that was interrupted
    /// before it completed; a read that ignored it would report an alias
    /// gone that the interrupted write still committed to keep. It is only
    /// ever read here, never applied: completing it is the edit path's job,
    /// under its own lock, so a read never turns into a write and never
    /// fails because one could not be made.
    #[cfg(windows)]
    fn read_intended_content(path: &Path) -> Result<String> {
        let pending = pending_path(path);
        match std::fs::read_to_string(&pending) {
            Ok(contents) => return Ok(contents),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                log::warn!(
                    "Ignoring unreadable pending hosts rewrite at {}: {error}",
                    pending.display()
                );
            }
        }
        match std::fs::read_to_string(path) {
            Ok(contents) => Ok(contents),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(String::new()),
            Err(error) => Err(error.into()),
        }
    }

    /// Whether anything differs from what was read.
    pub fn is_dirty(&self) -> bool {
        self.lines != self.original
    }

    /// The terminator a line kftray itself formats should use: whichever
    /// one most of the file's lines already have. A file with no lines yet
    /// (freshly created) follows the platform's own convention instead.
    fn dominant_crlf(&self) -> bool {
        if self.original.is_empty() {
            return cfg!(windows);
        }
        let crlf_count = self.original.iter().filter(|line| line.crlf).count();
        crlf_count * 2 > self.original.len()
    }

    fn begin_marker(tag: &str) -> String {
        format!("# DO NOT EDIT {tag} BEGIN")
    }

    fn end_marker(tag: &str) -> String {
        format!("# DO NOT EDIT {tag} END")
    }

    /// Indices of the begin and end markers of `tag`'s section.
    ///
    /// Half a section is an error, not an empty one: the aliases between a
    /// begin marker and a missing end marker still resolve, and reporting
    /// nothing would let a caller treat them as already removed.
    /// The section's marker lines, or `None` when the file has no section for
    /// the tag.
    ///
    /// A file with two sections for one tag is refused rather than read as
    /// its first: an edit would touch only that one, and a verification that
    /// followed would report an alias gone while the other section still
    /// resolves it. Nothing here writes a second section, so one is a file
    /// that was edited by hand and has to be repaired by hand.
    fn bounds(&self, tag: &str) -> Result<Option<(usize, usize)>> {
        let begin_marker = Self::begin_marker(tag);
        let end_marker = Self::end_marker(tag);
        let mut begins = self
            .lines
            .iter()
            .enumerate()
            .filter(|(_, line)| line.text.trim() == begin_marker)
            .map(|(index, _)| index);
        let mut ends = self
            .lines
            .iter()
            .enumerate()
            .filter(|(_, line)| line.text.trim() == end_marker)
            .map(|(index, _)| index);
        let (begin, end) = (begins.next(), ends.next());
        if begins.next().is_some() || ends.next().is_some() {
            return Err(HostsFileError::InvalidData(format!(
                "Duplicate section markers for tag '{tag}'"
            )));
        }
        match (begin, end) {
            (None, None) => Ok(None),
            (Some(begin), Some(end)) if begin < end => Ok(Some((begin, end))),
            (Some(_), Some(_)) => Err(HostsFileError::InvalidData(format!(
                "Reversed section markers for tag '{tag}'"
            ))),
            _ => Err(HostsFileError::InvalidData(format!(
                "Incomplete section markers for tag '{tag}'"
            ))),
        }
    }

    /// Every begin/end marker pair for `tag`, in file order.
    ///
    /// Unlike `bounds`, more than one pair is not an error: this is what a
    /// destructive removal uses to clean up a file a hand edit left with the
    /// tag duplicated, which an in-place mutation could not safely target.
    fn all_bounds(&self, tag: &str) -> Result<Vec<(usize, usize)>> {
        let begin_marker = Self::begin_marker(tag);
        let end_marker = Self::end_marker(tag);
        // A stack pairs each END with the nearest BEGIN still open, so
        // interleaved markers (BEGIN, BEGIN, END, END) nest rather than
        // cross: the inner pair is folded into the outer one below instead
        // of yielding two overlapping ranges a later drain could not apply
        // safely.
        let mut open: Vec<usize> = Vec::new();
        let mut top_level: Vec<(usize, usize)> = Vec::new();
        for (index, line) in self.lines.iter().enumerate() {
            let trimmed = line.text.trim();
            if trimmed == begin_marker {
                open.push(index);
            } else if trimmed == end_marker {
                let Some(begin) = open.pop() else {
                    return Err(HostsFileError::InvalidData(format!(
                        "Incomplete section markers for tag '{tag}'"
                    )));
                };
                if open.is_empty() {
                    top_level.push((begin, index));
                }
            }
        }
        if !open.is_empty() {
            return Err(HostsFileError::InvalidData(format!(
                "Incomplete section markers for tag '{tag}'"
            )));
        }
        Ok(top_level)
    }

    /// Folds every section for `tag` into one, in file order, when there is
    /// more than one.
    ///
    /// `bounds` refuses a duplicated tag outright because an in-place edit
    /// would not know which copy to touch. A destructive rewrite
    /// (`reconcile_owners`, `retain`) is not so constrained: it can fall
    /// back to merging every copy's body into the first one's place before
    /// it goes on to do its own work through `bounds`, rather than failing
    /// the whole operation over a file a hand edit left duplicated.
    pub fn merge_duplicate_sections(&mut self, tag: &str) -> Result<()> {
        let sections = self.all_bounds(tag)?;
        if sections.len() <= 1 {
            return Ok(());
        }
        let mut body: Vec<Line> = Vec::new();
        for &(begin, end) in &sections {
            body.extend(self.lines[begin + 1..end].iter().cloned());
        }
        for &(begin, end) in sections.iter().rev() {
            self.lines.drain(begin..=end);
            if begin > 0
                && begin <= self.lines.len()
                && self.lines[begin - 1].is_empty()
                && self.lines.get(begin).is_none_or(Line::is_empty)
            {
                self.lines.remove(begin - 1);
            }
        }
        self.set_body(tag, body)
    }

    /// Parses one line of a section. Comments and blank lines yield nothing.
    fn parse_line(line: &str) -> Option<ParsedLine> {
        // Everything from the first `#` is a comment. Ownership counts only
        // when that comment is the marker itself, so a note that happens to
        // mention the marker cannot make a foreign line look owned, and a real
        // alias is never parsed as comment words.
        let (fields, comment) = match line.split_once('#') {
            Some((fields, comment)) => (fields, Some(comment)),
            None => (line, None),
        };
        let owner = comment
            .and_then(|comment| {
                format!("#{comment}")
                    .strip_prefix(OWNER_MARKER.trim_start())
                    .map(ToOwned::to_owned)
            })
            .map(|owner| owner.trim().to_owned());
        let mut fields = fields.split_whitespace();
        let ip = fields.next()?.parse::<IpAddr>().ok()?;
        let hostnames: Vec<String> = fields.map(ToOwned::to_owned).collect();
        if hostnames.is_empty() {
            return None;
        }
        Some(ParsedLine {
            ip,
            hostnames,
            owner,
        })
    }

    fn format_line(ip: IpAddr, hostnames: &[String], owner: Option<&str>) -> String {
        match owner {
            Some(owner) => format!("{ip} {}{OWNER_MARKER}{owner}", hostnames.join(" ")),
            None => format!("{ip} {}", hostnames.join(" ")),
        }
    }

    /// Every alias inside `tag`'s section, in file order.
    pub fn section(&self, tag: &str) -> Result<Vec<SectionEntry>> {
        let Some((begin, end)) = self.bounds(tag)? else {
            return Ok(Vec::new());
        };
        Ok(self.entries_in(begin, end))
    }

    /// Every alias inside `tag`'s section(s), in file order, tolerating a
    /// duplicated tag by reading and concatenating every section's body
    /// instead of erroring the way `section` does through `bounds`.
    ///
    /// For a read-only caller that would otherwise have to fail outright on
    /// a file a hand edit (or an older bug) left duplicated, before a later
    /// write's own `merge_duplicate_sections` gets a chance to repair it.
    pub fn section_merging_duplicates(&self, tag: &str) -> Result<Vec<SectionEntry>> {
        Ok(self
            .all_bounds(tag)?
            .into_iter()
            .flat_map(|(begin, end)| self.entries_in(begin, end))
            .collect())
    }

    fn entries_in(&self, begin: usize, end: usize) -> Vec<SectionEntry> {
        self.lines[begin + 1..end]
            .iter()
            .filter_map(|line| Self::parse_line(&line.text))
            .flat_map(|parsed| {
                parsed
                    .hostnames
                    .into_iter()
                    .map(move |hostname| SectionEntry {
                        ip: parsed.ip,
                        hostname,
                        owner: parsed.owner.clone(),
                    })
            })
            .collect()
    }

    /// Replaces the body of `tag`'s section with `body`, creating the section
    /// at the end of the file when it does not exist and removing it, markers
    /// included, when `body` is empty.
    fn set_body(&mut self, tag: &str, body: Vec<Line>) -> Result<()> {
        let dominant = self.dominant_crlf();
        match self.bounds(tag)? {
            Some((begin, end)) => {
                if body.is_empty() {
                    self.lines.drain(begin..=end);
                    // The blank line that separated the section from what came
                    // before it goes with it, so repeated add-and-remove cycles
                    // do not grow the file.
                    if begin > 0
                        && begin <= self.lines.len()
                        && self.lines[begin - 1].is_empty()
                        && self.lines.get(begin).is_none_or(Line::is_empty)
                    {
                        self.lines.remove(begin - 1);
                    }
                } else {
                    self.lines.splice(begin + 1..end, body);
                }
            }
            None => {
                if body.is_empty() {
                    return Ok(());
                }
                if self.lines.last().is_some_and(|last| !last.is_empty()) {
                    self.lines.push(Line::new(String::new(), dominant));
                }
                self.lines
                    .push(Line::new(Self::begin_marker(tag), dominant));
                self.lines.extend(body);
                self.lines.push(Line::new(Self::end_marker(tag), dominant));
            }
        }
        Ok(())
    }

    /// Replaces `tag`'s section outright with one line per entry.
    pub fn replace_section(&mut self, tag: &str, entries: &[SectionEntry]) -> Result<()> {
        for entry in entries {
            validate_hostname(&entry.hostname)?;
            if let Some(owner) = &entry.owner {
                validate_owner(owner)?;
            }
        }
        let dominant = self.dominant_crlf();
        let body = entries
            .iter()
            .map(|entry| {
                Line::new(
                    Self::format_line(
                        entry.ip,
                        std::slice::from_ref(&entry.hostname),
                        entry.owner.as_deref(),
                    ),
                    dominant,
                )
            })
            .collect();
        self.set_body(tag, body)
    }

    /// Removes `tag`'s section whole, whoever wrote its lines.
    pub fn clear_section(&mut self, tag: &str) -> Result<()> {
        for (begin, end) in self.all_bounds(tag)?.into_iter().rev() {
            self.lines.drain(begin..=end);
            if begin > 0
                && begin <= self.lines.len()
                && self.lines[begin - 1].is_empty()
                && self.lines.get(begin).is_none_or(Line::is_empty)
            {
                self.lines.remove(begin - 1);
            }
        }
        Ok(())
    }

    /// Replaces the lines owned by `owners` with `entries` and leaves every
    /// other line of the section as it is.
    ///
    /// Passing no entries removes those owners. Returns the subset of
    /// `owners` that had lines before the change, so a caller can tell an id
    /// it never held apart from one it just took off disk.
    pub fn reconcile_owners(
        &mut self, tag: &str, owners: &[&str], entries: &[SectionEntry],
    ) -> Result<HashSet<String>> {
        for owner in owners {
            validate_owner(owner)?;
        }
        for entry in entries {
            validate_hostname(&entry.hostname)?;
            if let Some(owner) = &entry.owner {
                validate_owner(owner)?;
            }
        }
        self.merge_duplicate_sections(tag)?;
        let mut present = HashSet::new();
        let dominant = self.dominant_crlf();
        let mut body: Vec<Line> = match self.bounds(tag)? {
            Some((begin, end)) => self.lines[begin + 1..end]
                .iter()
                .filter(
                    |line| match Self::parse_line(&line.text).and_then(|parsed| parsed.owner) {
                        Some(owner) if owners.contains(&owner.as_str()) => {
                            present.insert(owner);
                            false
                        }
                        _ => true,
                    },
                )
                .cloned()
                .collect(),
            None => Vec::new(),
        };
        body.extend(entries.iter().map(|entry| {
            Line::new(
                Self::format_line(
                    entry.ip,
                    std::slice::from_ref(&entry.hostname),
                    entry.owner.as_deref(),
                ),
                dominant,
            )
        }));
        self.set_body(tag, body)?;
        Ok(present)
    }

    /// Keeps the aliases of `tag`'s section that `keep` accepts.
    ///
    /// A line none of whose aliases is rejected is preserved byte for byte;
    /// one with some rejected is rewritten with the rest.
    pub fn retain(&mut self, tag: &str, keep: impl Fn(&SectionEntry) -> bool) -> Result<()> {
        self.merge_duplicate_sections(tag)?;
        let Some((begin, end)) = self.bounds(tag)? else {
            return Ok(());
        };
        let dominant = self.dominant_crlf();
        let body: Vec<Line> = self.lines[begin + 1..end]
            .iter()
            .filter_map(|line| {
                let Some(parsed) = Self::parse_line(&line.text) else {
                    return Some(line.clone());
                };
                let kept: Vec<String> = parsed
                    .hostnames
                    .iter()
                    .filter(|hostname| {
                        keep(&SectionEntry {
                            ip: parsed.ip,
                            hostname: (*hostname).clone(),
                            owner: parsed.owner.clone(),
                        })
                    })
                    .cloned()
                    .collect();
                if kept.len() == parsed.hostnames.len() {
                    Some(line.clone())
                } else if kept.is_empty() {
                    None
                } else {
                    Some(Line::new(
                        Self::format_line(parsed.ip, &kept, parsed.owner.as_deref()),
                        dominant,
                    ))
                }
            })
            .collect();
        self.set_body(tag, body)
    }

    fn commit(&self, path: &Path) -> Result<bool> {
        if !self.is_dirty() {
            return Ok(false);
        }
        // The file's own trailing newline is kept only when the very last
        // line on disk is still, byte for byte, the last line being
        // written: anything appended or changed after it always ends in a
        // newline, matching every line before it.
        let omit_trailing_terminator =
            !self.ends_with_newline && self.lines.last() == self.original.last();
        let last_index = self.lines.len().saturating_sub(1);
        let mut content = Vec::new();
        for (index, line) in self.lines.iter().enumerate() {
            content.extend_from_slice(line.text.as_bytes());
            if index == last_index && omit_trailing_terminator {
                continue;
            }
            content.extend_from_slice(if line.crlf { b"\r\n" } else { b"\n" });
        }
        AtomicFileWriter::new(path).write_content(&content)?;
        Ok(true)
    }
}

/// Edits the system hosts file under its lock and writes it back once, only
/// if anything changed.
pub fn edit_hosts<T>(edit: impl FnOnce(&mut HostsDocument) -> Result<T>) -> Result<T> {
    edit_hosts_at(&get_default_hosts_path()?, edit)
}

/// [`edit_hosts`] against a specific file.
pub fn edit_hosts_at<T>(
    path: &Path, edit: impl FnOnce(&mut HostsDocument) -> Result<T>,
) -> Result<T> {
    validate_hosts_target_path(path)?;
    with_hosts_lock(path, true, || {
        let mut document = HostsDocument::load(path)?;
        let outcome = edit(&mut document)?;
        document.commit(path)?;
        Ok(outcome)
    })
}

/// Reads the system hosts file under its lock.
///
/// Locked like a write: the writer's fallback rewrites the file in place, and
/// a read in the middle of that would see an empty file and report aliases
/// gone that the completed write still holds.
pub fn read_hosts<T>(read: impl FnOnce(&HostsDocument) -> Result<T>) -> Result<T> {
    read_hosts_at(&get_default_hosts_path()?, read)
}

/// [`read_hosts`] against a specific file.
pub fn read_hosts_at<T>(path: &Path, read: impl FnOnce(&HostsDocument) -> Result<T>) -> Result<T> {
    validate_hosts_target_path(path)?;
    // A file that does not exist holds nothing, and a read must not be the
    // thing that creates it.
    if !path.exists() {
        return read(&HostsDocument {
            lines: Vec::new(),
            original: Vec::new(),
            ends_with_newline: true,
        });
    }
    with_hosts_lock(path, false, || read(&HostsDocument::load(path)?))
}

/// A set of aliases to write as one tagged section.
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

    pub fn add_entry<S: ToString>(&mut self, ip: IpAddr, hostname: S) -> Result<&mut Self> {
        let hostname = hostname.to_string();
        validate_hostname(&hostname)?;
        self.entries.push(SectionEntry {
            ip,
            hostname,
            owner: None,
        });
        Ok(self)
    }

    pub fn add_entries<I, S>(&mut self, ip: IpAddr, hostnames: I) -> Result<&mut Self>
    where
        I: IntoIterator<Item = S>,
        S: ToString,
    {
        for hostname in hostnames {
            self.add_entry(ip, hostname)?;
        }
        Ok(self)
    }

    /// Adds an entry that records which configuration owns it.
    ///
    /// Entries written without an owner cannot be told apart, and a writer
    /// that rebuilt a section from what it remembers would drop every line it
    /// did not write itself. The owner goes into a trailing comment, which the
    /// hosts file format ignores, so a writer can replace exactly its own
    /// lines and leave every other line alone.
    ///
    /// Owners are restricted to a plain token. The value ends up inside the
    /// file, and an owner carrying a line break or a `#` would otherwise turn
    /// into an active mapping, or comment out the one it was meant to mark.
    pub fn add_owned_entry<S: ToString>(
        &mut self, ip: IpAddr, hostname: S, owner: &str,
    ) -> Result<&mut Self> {
        validate_owner(owner)?;
        let hostname = hostname.to_string();
        validate_hostname(&hostname)?;
        self.entries.push(SectionEntry {
            ip,
            hostname,
            owner: Some(owner.to_owned()),
        });
        Ok(self)
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// Replaces this tag's section with the staged entries. No entries
    /// removes the section. Returns whether the file changed.
    pub fn write(&self) -> Result<bool> {
        self.write_to(get_default_hosts_path()?)
    }

    pub fn write_to<P: AsRef<Path>>(&self, path: P) -> Result<bool> {
        edit_hosts_at(path.as_ref(), |document| {
            document.replace_section(&self.tag, &self.entries)?;
            Ok(document.is_dirty())
        })
    }

    /// Reads a section from a specific file.
    pub fn read_section_from<P: AsRef<Path>>(&self, path: P) -> Result<Vec<SectionEntry>> {
        read_hosts_at(path.as_ref(), |document| document.section(&self.tag))
    }

    /// Replaces the lines owned by `owners` with the staged entries and leaves
    /// every other line alone, as one locked read-modify-write. See
    /// [`HostsDocument::reconcile_owners`].
    pub fn reconcile_owners_in<P: AsRef<Path>>(
        &self, path: P, owners: &[&str],
    ) -> Result<HashSet<String>> {
        for owner in owners {
            validate_owner(owner)?;
        }
        edit_hosts_at(path.as_ref(), |document| {
            document.reconcile_owners(&self.tag, owners, &self.entries)
        })
    }

    /// Reads this tag's section, keeps the entries `keep` accepts, and writes
    /// the result back as one locked operation.
    pub fn retain_section_in<P: AsRef<Path>>(
        &self, path: P, keep: impl Fn(&SectionEntry) -> bool,
    ) -> Result<bool> {
        edit_hosts_at(path.as_ref(), |document| {
            document.retain(&self.tag, keep)?;
            Ok(document.is_dirty())
        })
    }
}

struct AtomicFileWriter<'a> {
    target_path: &'a Path,
}

impl<'a> AtomicFileWriter<'a> {
    fn new(path: &'a Path) -> Self {
        Self { target_path: path }
    }

    #[cfg(not(windows))]
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

    /// Written in place on Windows, after the content is committed.
    ///
    /// The lock lives on an open handle, and a handle opened by the standard
    /// library shares deletion, so a rename over the file would succeed and
    /// leave the lock on a retired file while the next writer locks the
    /// replacement. In place, the locked handle stays the current file. The
    /// file is truncated before it is rewritten, so the new content is first
    /// committed next to it, atomically, and the in-place write is repeated
    /// from that copy if it does not complete. Retiring the copy is part of
    /// the write, but the write already succeeded and was fsynced by the
    /// time it happens: a failure to remove it is only ever logged, not
    /// reported as a failed write.
    ///
    /// The pending copy is staged next to the hosts file itself; a caller
    /// unable to write there is not privileged enough to write the hosts
    /// file either, and gets that `PermissionDenied` back rather than a
    /// fallback location a less privileged process could also reach.
    #[cfg(windows)]
    fn write_content(&self, content: &[u8]) -> Result<()> {
        let pending = pending_path(self.target_path);
        let mut staging = pending.as_os_str().to_owned();
        staging.push(".tmp");
        let staging = PathBuf::from(staging);

        validate_hosts_path(&staging)?;
        if let Err(error) = std::fs::remove_file(&staging)
            && error.kind() != io::ErrorKind::NotFound
        {
            log::warn!(
                "Removing a leftover staged hosts rewrite at {}: {error}",
                staging.display()
            );
        }
        let mut staged = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&staging)?;
        staged.write_all(content)?;
        staged.sync_all()?;
        drop(staged);

        validate_hosts_path(&pending)?;
        // A leftover pending copy can already be here from an earlier write
        // that crashed between publishing it and removing it, or whose
        // removal merely failed and was only logged. `rename` needs the
        // destination clear; clearing it here treats that leftover as
        // ordinary recovery debris rather than letting it break every write
        // that follows.
        if let Err(error) = std::fs::remove_file(&pending)
            && error.kind() != io::ErrorKind::NotFound
        {
            log::warn!(
                "Removing a leftover pending hosts rewrite at {}: {error}",
                pending.display()
            );
        }
        std::fs::rename(&staging, &pending)?;
        self.write_directly(content)?;
        OpenOptions::new()
            .write(true)
            .open(self.target_path)?
            .sync_all()?;
        if let Err(error) = std::fs::remove_file(&pending) {
            log::warn!(
                "Hosts file write to {} succeeded, but the pending copy at {} could not be \
                 removed: {error}",
                self.target_path.display(),
                pending.display()
            );
        }
        Ok(())
    }

    #[cfg(not(windows))]
    fn try_atomic_write(&self, content: &[u8]) -> Result<()> {
        let temp_path = self.create_temp_path()?;

        std::fs::copy(self.target_path, &temp_path)?;

        #[cfg(target_os = "linux")]
        self.preserve_selinux_context(&temp_path);

        self.write_file(&temp_path, content)?;
        std::fs::rename(&temp_path, self.target_path)?;

        Ok(())
    }

    #[cfg(not(windows))]
    fn create_temp_path(&self) -> Result<PathBuf> {
        let parent = self.target_path.parent().ok_or_else(|| {
            HostsFileError::InvalidPath("Path has no parent directory".to_string())
        })?;

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
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
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(path)?;
        file.write_all(content)?;
        // Durable before it is renamed into place or relied on for recovery:
        // a copy that is published before its bytes reach the disk is what a
        // crash would restore from.
        file.sync_all()?;
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

/// Rejects a directory or a symlink at one of this crate's own pending or
/// staging siblings (`.kftray-pending`, `.tmp`): those locations are never
/// created ahead of time by anything but this crate, and a symlink/reparse
/// point already there could only have been planted by another party. Its
/// lexical parent being user-writable would let an unprivileged process
/// plant one beside the real hosts file for a privileged recovery to
/// traverse, so it is refused outright rather than resolved.
///
/// A missing path is not rejected: a custom path may still be created on
/// first write, and a read of one reports an empty document.
#[cfg_attr(not(windows), allow(dead_code))]
fn validate_hosts_path(path: &Path) -> Result<()> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    if metadata.file_type().is_symlink() {
        return Err(HostsFileError::InvalidPath(
            "Hosts path must not be a symlink".to_string(),
        ));
    }
    if metadata.is_dir() {
        return Err(HostsFileError::InvalidPath(
            "Expected file path, got directory".to_string(),
        ));
    }
    Ok(())
}

/// Validates the hosts file's own path (the argument to [`edit_hosts_at`] /
/// [`read_hosts_at`]) before it is opened.
///
/// Unlike the pending/staging siblings [`validate_hosts_path`] guards, this
/// path is not something this crate creates: platforms such as NixOS ship
/// `/etc/hosts` as a symlink to a generated target, and container/managed
/// images do the same, so rejecting a symlink outright here made every read
/// and write fail on those systems. A symlink is followed and its resolved
/// target validated instead; only a directory, lexically or through the
/// link, is rejected. A dangling symlink resolves to "not found", the same
/// as a missing path.
fn validate_hosts_target_path(path: &Path) -> Result<()> {
    match std::fs::metadata(path) {
        Ok(metadata) => {
            if metadata.is_dir() {
                return Err(HostsFileError::InvalidPath(
                    "Expected file path, got directory".to_string(),
                ));
            }
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    #[test]
    fn a_duplicated_section_is_refused_rather_than_read_as_its_first() {
        let (_temp_file, temp_path) = tempfile::NamedTempFile::new().unwrap().into_parts();
        let mut file = HostsFile::new("test");
        file.add_owned_entry([127, 0, 0, 1].into(), "a.local", "1")
            .unwrap();
        file.write_to(&temp_path).unwrap();
        // A copy of the whole section pasted below it, as a hand edit could.
        let content = std::fs::read_to_string(&temp_path).unwrap();
        std::fs::write(&temp_path, format!("{content}{content}")).unwrap();

        let error = read_hosts_at(&temp_path, |document| document.section("test"))
            .expect_err("two sections for one tag cannot be edited safely");
        assert!(
            error.to_string().contains("Duplicate section markers"),
            "{error}"
        );
    }

    #[test]
    fn clear_section_removes_every_duplicated_section() {
        let (_temp_file, temp_path) = tempfile::NamedTempFile::new().unwrap().into_parts();
        let mut file = HostsFile::new("test");
        file.add_owned_entry([127, 0, 0, 1].into(), "a.local", "1")
            .unwrap();
        file.write_to(&temp_path).unwrap();
        // A copy of the whole section pasted below it, as a hand edit could;
        // `bounds` refuses this, but the destructive removal path must not.
        let content = std::fs::read_to_string(&temp_path).unwrap();
        std::fs::write(&temp_path, format!("{content}{content}")).unwrap();

        edit_hosts_at(&temp_path, |document| document.clear_section("test")).unwrap();

        let remaining = std::fs::read_to_string(&temp_path).unwrap();
        assert!(
            !remaining.contains("DO NOT EDIT test"),
            "clear_section must remove every matching section, not just the first: {remaining}"
        );
    }

    #[test]
    fn clear_section_handles_interleaved_markers_without_panicking() {
        let (_temp_file, temp_path) = tempfile::NamedTempFile::new().unwrap().into_parts();
        // BEGIN, BEGIN, END, END: what a hand edit that pasted one section
        // inside another leaves behind. The two pairs nest rather than
        // cross, so this must fold into one removal instead of the
        // overlapping (0, 2) / (1, 3) ranges a naive index-order zip would
        // produce, which `clear_section`'s high-to-low drain cannot apply
        // safely.
        let content = format!(
            "{}\n{}\n127.0.0.1 a.local\n127.0.0.1 b.local\n{}\n{}\n",
            HostsDocument::begin_marker("test"),
            HostsDocument::begin_marker("test"),
            HostsDocument::end_marker("test"),
            HostsDocument::end_marker("test"),
        );
        std::fs::write(&temp_path, content).unwrap();

        edit_hosts_at(&temp_path, |document| document.clear_section("test")).unwrap();

        let remaining = std::fs::read_to_string(&temp_path).unwrap();
        assert!(
            !remaining.contains("DO NOT EDIT test"),
            "clear_section must remove interleaved sections without panicking: {remaining}"
        );
    }

    #[test]
    fn reconciling_owners_merges_duplicated_sections_instead_of_failing() {
        let (_temp_file, temp_path) = tempfile::NamedTempFile::new().unwrap().into_parts();
        let mut file = HostsFile::new("test");
        file.add_owned_entry([127, 0, 0, 1].into(), "a.local", "1")
            .unwrap();
        file.write_to(&temp_path).unwrap();
        // A copy of the whole section pasted below it, as a hand edit could;
        // `bounds` refuses this, but a destructive rewrite must fall back to
        // merging instead of failing the whole stop.
        let content = std::fs::read_to_string(&temp_path).unwrap();
        std::fs::write(&temp_path, format!("{content}{content}")).unwrap();

        let mut next = HostsFile::new("test");
        next.add_owned_entry([127, 0, 0, 2].into(), "b.local", "2")
            .unwrap();
        let present = next.reconcile_owners_in(&temp_path, &["1"]).unwrap();
        assert_eq!(present, HashSet::from(["1".to_owned()]));

        let remaining = std::fs::read_to_string(&temp_path).unwrap();
        assert_eq!(
            remaining.matches("DO NOT EDIT test BEGIN").count(),
            1,
            "the duplicated sections must be merged into one: {remaining}"
        );
        assert!(!remaining.contains("a.local"), "{remaining}");
        assert!(remaining.contains("b.local"), "{remaining}");
    }

    #[test]
    fn a_read_style_lock_never_creates_a_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hosts");

        with_hosts_lock(&path, false, || Ok(())).unwrap();

        assert!(
            !path.exists(),
            "a read-style lock must never create the hosts file it did not find"
        );
    }

    #[test]
    fn the_lock_is_released_after_a_panic_in_work() {
        let (_temp_file, temp_path) = tempfile::NamedTempFile::new().unwrap().into_parts();

        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            with_hosts_lock(&temp_path, true, || -> Result<()> {
                panic!("work panics before returning");
            })
        }));
        assert!(panicked.is_err(), "the closure above must have panicked");

        // A lock left held by the panicked call would make this block until
        // the 15s wait budget in `open_locked` expires instead of taking it
        // right away.
        let start = std::time::Instant::now();
        with_hosts_lock(&temp_path, true, || Ok(())).unwrap();
        assert!(
            start.elapsed() < std::time::Duration::from_secs(1),
            "the lock from the panicked call must be released immediately, not held until its \
             fd closes"
        );
    }

    #[test]
    #[cfg(unix)]
    fn a_symlinked_hosts_path_is_followed_to_its_target() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real-hosts");
        std::fs::write(&real, "127.0.0.1 real.local\n").unwrap();
        let link = dir.path().join("hosts-link");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        // NixOS and similar managed systems ship `/etc/hosts` as a symlink
        // to a generated target; a read through the link must resolve it
        // rather than being rejected outright.
        let lines: Vec<String> = read_hosts_at(&link, |document| {
            Ok(document
                .lines
                .iter()
                .map(|line| line.text.clone())
                .collect())
        })
        .unwrap();
        assert_eq!(lines, vec!["127.0.0.1 real.local".to_string()]);
    }

    #[test]
    #[cfg(unix)]
    fn a_hosts_path_resolving_to_a_directory_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let target_dir = dir.path().join("real-dir");
        std::fs::create_dir(&target_dir).unwrap();
        let link = dir.path().join("hosts-link");
        std::os::unix::fs::symlink(&target_dir, &link).unwrap();

        let error = read_hosts_at(&link, |document| document.section("test"))
            .expect_err("a path resolving to a directory must be rejected, symlink or not");
        assert!(error.to_string().contains("directory"), "{error}");
    }

    #[test]
    #[cfg(unix)]
    fn a_symlinked_pending_sibling_is_refused() {
        // Windows recovery (`open_locked`) and staging (`AtomicFileWriter`)
        // both check their `.kftray-pending`/`.tmp` sibling with
        // `validate_hosts_path`, which stays strict about symlinks there
        // even though the hosts path itself now resolves them: those
        // locations are only ever created fresh by this crate, so a symlink
        // found there was planted by someone else. This exercises that
        // shared check on a `.kftray-pending`-named symlink without
        // requiring a Windows target.
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real-hosts");
        std::fs::write(&real, "127.0.0.1 real.local\n").unwrap();
        let pending = dir.path().join("hosts.kftray-pending");
        std::os::unix::fs::symlink(&real, &pending).unwrap();

        let error =
            validate_hosts_path(&pending).expect_err("a symlinked pending sibling must be refused");
        assert!(error.to_string().contains("symlink"), "{error}");
    }

    #[test]
    #[cfg(windows)]
    fn a_leftover_pending_file_does_not_break_the_next_write() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hosts");
        std::fs::write(&path, "original\n").unwrap();
        // As if an earlier write crashed after publishing its pending copy
        // but before removing it, or the removal merely failed and was
        // only logged.
        std::fs::write(pending_path(&path), "stale-pending\n").unwrap();

        AtomicFileWriter::new(&path)
            .write_content(b"fresh\n")
            .unwrap();

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "fresh\n");
        assert!(
            !pending_path(&path).exists(),
            "the write's own pending copy must be cleaned up: {}",
            pending_path(&path).display()
        );
    }

    #[test]
    #[cfg(windows)]
    fn open_locked_recovery_applies_the_pending_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hosts");
        std::fs::write(&path, "original\n").unwrap();

        let pending = pending_path(&path);
        std::fs::write(&pending, "fresh-pending\n").unwrap();

        open_locked(&path, true).unwrap();

        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "fresh-pending\n",
            "recovery must apply the pending rewrite sibling to the hosts file"
        );
        assert!(
            !pending.exists(),
            "the pending file must be removed once recovery completes"
        );
    }

    #[test]
    #[cfg(windows)]
    fn a_pending_rewrite_for_one_path_is_never_applied_to_another() {
        let dir = tempfile::tempdir().unwrap();
        let path_a = dir.path().join("hosts_a");
        let path_b = dir.path().join("hosts_b");
        std::fs::write(&path_a, "a-original\n").unwrap();
        std::fs::write(&path_b, "b-original\n").unwrap();

        // As if a write to A was interrupted after the copy was committed
        // but before it was applied.
        std::fs::write(pending_path(&path_a), "a-pending\n").unwrap();

        let a_lines: Vec<String> = read_hosts_at(&path_a, |document| {
            Ok(document
                .lines
                .iter()
                .map(|line| line.text.clone())
                .collect())
        })
        .unwrap();
        let b_lines: Vec<String> = read_hosts_at(&path_b, |document| {
            Ok(document
                .lines
                .iter()
                .map(|line| line.text.clone())
                .collect())
        })
        .unwrap();

        assert_eq!(
            a_lines,
            vec!["a-pending".to_owned()],
            "a read sees the pending rewrite as the intended state"
        );
        assert_eq!(
            b_lines,
            vec!["b-original".to_owned()],
            "a pending rewrite staged for a different path is never applied here"
        );
        // The read never writes: the file and the pending copy are both
        // exactly as they were.
        assert_eq!(std::fs::read_to_string(&path_a).unwrap(), "a-original\n");
        assert_eq!(
            std::fs::read_to_string(pending_path(&path_a)).unwrap(),
            "a-pending\n"
        );
    }

    #[test]
    fn reconciling_owners_leaves_every_other_line_alone() {
        let (_temp_file, temp_path) = tempfile::NamedTempFile::new().unwrap().into_parts();

        // What an earlier run, and another writer, left behind: two owned
        // lines and one unmarked one.
        let mut earlier = HostsFile::new("test");
        earlier
            .add_owned_entry([127, 0, 0, 1].into(), "a.local", "1")
            .unwrap()
            .add_owned_entry([127, 0, 0, 1].into(), "b.local", "2")
            .unwrap()
            .add_entry([127, 0, 0, 1].into(), "plain.local")
            .unwrap();
        earlier.write_to(&temp_path).unwrap();

        // Owner 1 changes its alias; owner 3 appears; owner 2 is untouched.
        let mut next = HostsFile::new("test");
        next.add_owned_entry([127, 0, 0, 2].into(), "a2.local", "1")
            .unwrap()
            .add_owned_entry([127, 0, 0, 3].into(), "c.local", "3")
            .unwrap();
        let present = next.reconcile_owners_in(&temp_path, &["1", "3"]).unwrap();
        assert_eq!(
            present,
            HashSet::from(["1".to_owned()]),
            "only the owner that already had a line is reported present"
        );

        let entries = HostsFile::new("test")
            .read_section_from(&temp_path)
            .unwrap();
        let mut aliases: Vec<(String, Option<String>)> = entries
            .into_iter()
            .map(|entry| (entry.hostname, entry.owner))
            .collect();
        aliases.sort();
        assert_eq!(
            aliases,
            vec![
                ("a2.local".to_owned(), Some("1".to_owned())),
                ("b.local".to_owned(), Some("2".to_owned())),
                ("c.local".to_owned(), Some("3".to_owned())),
                ("plain.local".to_owned(), None),
            ],
            "the old line of a rewritten owner is gone, everything else survives"
        );

        // Staging nothing removes the named owners and nothing else.
        let present = HostsFile::new("test")
            .reconcile_owners_in(&temp_path, &["2", "missing"])
            .unwrap();
        assert_eq!(present, HashSet::from(["2".to_owned()]));
        let remaining: Vec<String> = HostsFile::new("test")
            .read_section_from(&temp_path)
            .unwrap()
            .into_iter()
            .map(|entry| entry.hostname)
            .collect();
        assert_eq!(remaining, vec!["plain.local", "a2.local", "c.local"]);
    }

    #[test]
    fn an_owner_cannot_change_the_shape_of_the_file() {
        let mut hosts_file = HostsFile::new("test");
        // An id with a line break would end the comment and start a mapping.
        assert!(
            hosts_file
                .add_owned_entry([127, 0, 0, 1].into(), "a.local", "42\n127.0.0.1 injected")
                .is_err()
        );
        assert!(
            hosts_file
                .add_owned_entry([127, 0, 0, 1].into(), "a.local", "42 # not-mine")
                .is_err()
        );
        assert!(
            hosts_file
                .add_owned_entry([127, 0, 0, 1].into(), "a.local\nevil", "42")
                .is_err()
        );
        assert!(
            hosts_file
                .add_owned_entry([127, 0, 0, 1].into(), "a.local", "42-https-local")
                .is_ok()
        );
        assert!(
            HostsFile::new("test")
                .reconcile_owners_in("/nonexistent", &["42\n"])
                .is_err(),
            "removal is validated too, or a bad id could match a comment line"
        );
    }

    #[test]
    fn add_entry_rejects_a_bad_hostname() {
        let mut hosts_file = HostsFile::new("test");
        assert!(
            hosts_file
                .add_entry([127, 0, 0, 1].into(), "a.local#injected")
                .is_err(),
            "a `#` would start a comment and swallow the rest of the line"
        );
        assert!(
            hosts_file
                .add_entry([127, 0, 0, 1].into(), "a.local evil")
                .is_err(),
            "whitespace would not stay on the hostname's own column"
        );
        assert!(hosts_file.is_empty(), "no rejected hostname is staged");
        assert!(
            hosts_file
                .add_entries([127, 0, 0, 1].into(), ["good.local", "bad host"])
                .is_err(),
            "add_entries must validate every hostname it stages"
        );
    }

    #[test]
    fn aliases_sharing_an_address_stay_on_their_own_lines() {
        let (_temp_file, temp_path) = tempfile::NamedTempFile::new().unwrap().into_parts();

        let mut hosts_file = HostsFile::new("test");
        // The SSL aliases of one configuration always share 127.0.0.1.
        hosts_file
            .add_owned_entry([127, 0, 0, 1].into(), "a.local", "1")
            .unwrap();
        hosts_file
            .add_owned_entry([127, 0, 0, 1].into(), "b.local", "2")
            .unwrap();
        hosts_file
            .add_entry([127, 0, 0, 1].into(), "plain.local")
            .unwrap();
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
        hosts_file
            .add_owned_entry([127, 0, 0, 8].into(), "round.local", "9001")
            .unwrap();
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
        hosts_file
            .add_entry([1, 1, 1, 1].into(), "example.com")
            .unwrap();

        assert!(hosts_file.write_to(&temp_path).unwrap());
        assert!(!hosts_file.write_to(&temp_path).unwrap());

        let contents = std::fs::read_to_string(&temp_path).unwrap();
        assert!(contents.contains("preexisting\ncontent"));
        assert!(contents.contains("# DO NOT EDIT test BEGIN"));
        assert!(contents.contains("1.1.1.1 example.com"));
        assert!(contents.contains("# DO NOT EDIT test END"));
    }

    #[test]
    fn a_missing_custom_hosts_file_is_created_on_first_write() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hosts");

        let mut hosts = HostsFile::new("test");
        hosts
            .add_entry([127, 0, 0, 1].into(), "fresh.local")
            .unwrap();
        assert!(hosts.write_to(&path).unwrap());
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .contains("127.0.0.1 fresh.local")
        );

        // A read of a path that still does not exist reports an empty section
        // rather than creating anything on its own.
        let absent = dir.path().join("never");
        assert!(
            HostsFile::new("test")
                .read_section_from(&absent)
                .unwrap()
                .is_empty()
        );
        assert!(!absent.exists(), "a read must not create the file");
    }

    #[test]
    fn test_fluent_api() {
        let mut hosts_file = HostsFile::new("test");
        hosts_file
            .add_entry([127, 0, 0, 1].into(), "localhost")
            .unwrap()
            .add_entries([192, 168, 1, 1].into(), ["router", "gateway"])
            .unwrap();

        // One entry per hostname: aliases of one address need their own lines
        // so an owner comment cannot swallow the ones after it.
        assert_eq!(hosts_file.entries.len(), 3);
    }

    #[test]
    fn untouched_lines_survive_byte_for_byte_and_a_no_op_never_writes() {
        let (mut temp_file, temp_path) = tempfile::NamedTempFile::new().unwrap().into_parts();
        temp_file
            .write_all(
                b"127.0.0.1 localhost\n\n\
                  # DO NOT EDIT test BEGIN\n\
                  # a hand-written note\n\
                  127.0.0.1   other.local   # note\n\
                  127.0.0.2 two.local three.local # kftray-id=9\n\
                  \n\
                  # DO NOT EDIT test END\n",
            )
            .unwrap();
        let before = std::fs::read_to_string(&temp_path).unwrap();
        let modified = std::fs::metadata(&temp_path).unwrap().modified().unwrap();

        // Removing an owner that is not there is a no-op: nothing is written,
        // which an unprivileged caller depends on to find out it owns nothing.
        let present = HostsFile::new("test")
            .reconcile_owners_in(&temp_path, &["missing"])
            .unwrap();
        assert!(present.is_empty());
        assert_eq!(std::fs::read_to_string(&temp_path).unwrap(), before);
        assert_eq!(
            std::fs::metadata(&temp_path).unwrap().modified().unwrap(),
            modified
        );

        // Removing owner 9 keeps the note, the spacing and the blank line of
        // everything else exactly as they were.
        HostsFile::new("test")
            .reconcile_owners_in(&temp_path, &["9"])
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(&temp_path).unwrap(),
            "127.0.0.1 localhost\n\n\
             # DO NOT EDIT test BEGIN\n\
             # a hand-written note\n\
             127.0.0.1   other.local   # note\n\
             \n\
             # DO NOT EDIT test END\n"
        );
    }

    #[test]
    fn retaining_rewrites_only_the_lines_it_changes() {
        let (mut temp_file, temp_path) = tempfile::NamedTempFile::new().unwrap().into_parts();
        temp_file
            .write_all(
                b"# DO NOT EDIT test BEGIN\n\
                  127.0.0.1  keep.local   # note\n\
                  127.0.0.2 gone.local stay.local\n\
                  127.0.0.3 all-gone.local\n\
                  # DO NOT EDIT test END\n",
            )
            .unwrap();

        HostsFile::new("test")
            .retain_section_in(&temp_path, |entry| {
                !matches!(entry.hostname.as_str(), "gone.local" | "all-gone.local")
            })
            .unwrap();

        assert_eq!(
            std::fs::read_to_string(&temp_path).unwrap(),
            "# DO NOT EDIT test BEGIN\n\
             127.0.0.1  keep.local   # note\n\
             127.0.0.2 stay.local\n\
             # DO NOT EDIT test END\n",
            "an untouched line keeps its spacing and note; a partly kept line is rewritten; a \
             fully rejected line goes"
        );
    }

    #[test]
    fn removing_the_last_alias_removes_the_section_and_its_separator() {
        let (mut temp_file, temp_path) = tempfile::NamedTempFile::new().unwrap().into_parts();
        temp_file.write_all(b"127.0.0.1 localhost\n").unwrap();

        let mut hosts = HostsFile::new("test");
        hosts
            .add_owned_entry([127, 0, 0, 1].into(), "only.local", "1")
            .unwrap();
        hosts.reconcile_owners_in(&temp_path, &["1"]).unwrap();
        HostsFile::new("test")
            .reconcile_owners_in(&temp_path, &["1"])
            .unwrap();

        assert_eq!(
            std::fs::read_to_string(&temp_path).unwrap(),
            "127.0.0.1 localhost\n",
            "add-and-remove cycles must not grow the file"
        );
    }

    #[test]
    #[cfg(unix)]
    fn a_bounded_retry_reports_a_timeout_instead_of_spinning_forever() {
        let mut attempts = 0;
        let result: Result<()> =
            retry_bounded("test", 3, std::time::Duration::from_millis(0), || {
                attempts += 1;
                Ok(None)
            });

        assert_eq!(attempts, 3, "every attempt runs before giving up");
        let error = result.expect_err("exhausting every attempt is a timeout, not success");
        assert!(error.to_string().contains("Timed out"), "{error}");
    }

    #[test]
    fn an_untouched_crlf_line_keeps_its_terminator_after_an_edit() {
        let (mut temp_file, temp_path) = tempfile::NamedTempFile::new().unwrap().into_parts();
        temp_file
            .write_all(
                b"127.0.0.1 localhost\r\n\
                  # DO NOT EDIT test BEGIN\r\n\
                  127.0.0.1 old.local # kftray-id=1\r\n\
                  # DO NOT EDIT test END\r\n",
            )
            .unwrap();

        let mut next = HostsFile::new("test");
        next.add_owned_entry([127, 0, 0, 2].into(), "new.local", "2")
            .unwrap();
        next.reconcile_owners_in(&temp_path, &["1"]).unwrap();

        assert_eq!(
            std::fs::read_to_string(&temp_path).unwrap(),
            "127.0.0.1 localhost\r\n\
             # DO NOT EDIT test BEGIN\r\n\
             127.0.0.2 new.local # kftray-id=2\r\n\
             # DO NOT EDIT test END\r\n",
            "an untouched line outside the section, and the file's own CRLF terminator, must \
             survive an edit made inside it rather than being normalised to LF"
        );
    }
}
