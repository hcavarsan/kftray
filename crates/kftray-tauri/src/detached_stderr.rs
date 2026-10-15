use std::io::{
    IsTerminal,
    Read,
};

const READ_CHUNK_BYTES: usize = 4096;
const MAX_PENDING_LINE_BYTES: usize = 8192;
const MAX_FORWARDED_BYTES: usize = 1_000_000;

pub fn redirect_if_detached() {
    if std::io::stderr().is_terminal() {
        return;
    }
    let (read, write) = match imp::pipe() {
        Ok(ends) => ends,
        Err(e) => {
            log::warn!("failed to capture detached stderr: {e}");
            return;
        }
    };
    let _ = std::thread::Builder::new()
        .name("stderr-capture".to_owned())
        .spawn(move || {
            imp::attach(write);
            forward(read);
        });
}

struct LineAssembler {
    pending: Vec<u8>,
}

impl LineAssembler {
    fn new() -> Self {
        Self {
            pending: Vec::with_capacity(READ_CHUNK_BYTES),
        }
    }

    fn push(&mut self, bytes: &[u8], out: &mut Vec<Vec<u8>>) {
        let mut start = 0;
        while start < bytes.len() {
            let len = bytes[start..]
                .iter()
                .position(|&b| b == b'\n')
                .map_or(bytes.len() - start, |at| at + 1);
            self.pending.extend_from_slice(&bytes[start..start + len]);
            start += len;
            if (self.pending.last() == Some(&b'\n') || self.pending.len() >= MAX_PENDING_LINE_BYTES)
                && let Some(line) = self.take_line()
            {
                out.push(line);
            }
        }
    }

    fn finish(&mut self, out: &mut Vec<Vec<u8>>) {
        if let Some(line) = self.take_line() {
            out.push(line);
        }
    }

    fn take_line(&mut self) -> Option<Vec<u8>> {
        while matches!(self.pending.last(), Some(b'\n' | b'\r')) {
            self.pending.pop();
        }
        if self.pending.is_empty() {
            return None;
        }
        Some(std::mem::take(&mut self.pending))
    }
}

fn forward<R: Read>(mut read: R) {
    let mut assembler = LineAssembler::new();
    let mut chunk = [0u8; READ_CHUNK_BYTES];
    let mut lines = Vec::new();
    let mut forwarded = 0usize;
    let mut announced = false;
    loop {
        match read.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                assembler.push(&chunk[..n], &mut lines);
                for line in lines.drain(..) {
                    if forwarded >= MAX_FORWARDED_BYTES {
                        if !announced {
                            announced = true;
                            log::warn!(
                                target: "stderr",
                                "stderr capture limit reached, further output is dropped"
                            );
                        }
                        continue;
                    }
                    let line = String::from_utf8_lossy(&line);
                    forwarded += line.len();
                    log::warn!(target: "stderr", "{line}");
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => break,
        }
    }
    assembler.finish(&mut lines);
    for line in lines {
        if forwarded < MAX_FORWARDED_BYTES {
            let line = String::from_utf8_lossy(&line);
            forwarded += line.len();
            log::warn!(target: "stderr", "{line}");
        }
    }
}

#[cfg(unix)]
mod imp {
    use std::fs::File;
    use std::io;
    use std::os::fd::{
        AsRawFd,
        FromRawFd,
        OwnedFd,
    };

    pub struct WriteEnd(OwnedFd);

    pub fn pipe() -> anyhow::Result<(File, WriteEnd)> {
        let mut fds = [0i32; 2];
        if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
            return Err(io::Error::last_os_error().into());
        }
        let read = unsafe { OwnedFd::from_raw_fd(fds[0]) };
        let write = unsafe { OwnedFd::from_raw_fd(fds[1]) };
        let fd = read.as_raw_fd();
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
        if flags >= 0 {
            unsafe { libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) };
        }
        Ok((File::from(read), WriteEnd(write)))
    }

    pub fn attach(write: WriteEnd) {
        let fd = write.0.as_raw_fd();
        if fd == libc::STDERR_FILENO {
            std::mem::forget(write);
            return;
        }
        if unsafe { libc::dup2(fd, libc::STDERR_FILENO) } < 0 {
            log::warn!("failed to redirect stderr: {}", io::Error::last_os_error());
        }
    }
}

#[cfg(windows)]
mod imp {
    use std::fs::File;
    use std::mem::size_of;
    use std::os::windows::io::{
        AsRawHandle,
        FromRawHandle,
        OwnedHandle,
    };

    use windows::Win32::Foundation::{
        HANDLE,
        HANDLE_FLAG_INHERIT,
        HANDLE_FLAGS,
        SetHandleInformation,
    };
    use windows::Win32::Security::SECURITY_ATTRIBUTES;
    use windows::Win32::System::Console::{
        STD_ERROR_HANDLE,
        SetStdHandle,
    };
    use windows::Win32::System::Pipes::CreatePipe;

    pub struct WriteEnd(OwnedHandle);

    pub fn pipe() -> anyhow::Result<(File, WriteEnd)> {
        let mut read = HANDLE::null();
        let mut write = HANDLE::null();
        let attrs = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: std::ptr::null_mut(),
            bInheritHandle: true.into(),
        };
        unsafe {
            CreatePipe(&mut read, &mut write, Some(&attrs), 0)?;
            let _ = SetHandleInformation(read, HANDLE_FLAG_INHERIT.0, HANDLE_FLAGS(0));
        }
        let read = OwnedHandle::from_raw_handle(read.0);
        let write = OwnedHandle::from_raw_handle(write.0);
        Ok((File::from(read), WriteEnd(write)))
    }

    pub fn attach(write: WriteEnd) {
        let handle = HANDLE(write.0.as_raw_handle());
        if let Err(e) = unsafe { SetStdHandle(STD_ERROR_HANDLE, handle) } {
            log::warn!("failed to redirect stderr: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collect(parts: &[&[u8]]) -> Vec<String> {
        let mut assembler = LineAssembler::new();
        let mut out = Vec::new();
        for part in parts {
            assembler.push(part, &mut out);
        }
        assembler.finish(&mut out);
        out.into_iter()
            .map(|line| String::from_utf8_lossy(&line).into_owned())
            .collect()
    }

    #[test]
    fn splits_complete_lines() {
        assert_eq!(collect(&[b"one\ntwo\n"]), vec!["one", "two"]);
    }

    #[test]
    fn buffers_partial_lines_across_reads() {
        assert_eq!(collect(&[b"par", b"tial\n"]), vec!["partial"]);
    }

    #[test]
    fn skips_blank_lines() {
        assert!(collect(&[b"\n\n\n"]).is_empty());
    }

    #[test]
    fn strips_carriage_returns() {
        assert_eq!(collect(&[b"crlf\r\n"]), vec!["crlf"]);
    }

    #[test]
    fn flushes_trailing_partial_line_at_eof() {
        assert_eq!(collect(&[b"tail"]), vec!["tail"]);
    }

    #[test]
    fn emits_oversized_line_without_newline() {
        let part = vec![b'x'; MAX_PENDING_LINE_BYTES];
        let lines = collect(&[&part, b"\nrest\n"]);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].len(), MAX_PENDING_LINE_BYTES);
        assert_eq!(lines[1], "rest");
    }
}
