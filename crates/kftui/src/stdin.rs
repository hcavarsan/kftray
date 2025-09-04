// workaround for crossterm stdin handling issue on macos
//
// crossterm can't properly handle stdin after it's been consumed by piped input
// because kqueue doesn't work with /dev/tty on macos. this is a known
// limitation described in crossterm issues https://github.com/crossterm-rs/crossterm/issues/396 and https://github.com/crossterm-rs/crossterm/issues/996.
//
// the solution is to find the actual controlling terminal device (like
// /dev/ttys000) and redirect stdin to that device using unsafe dup2 syscall.
// this allows the tui to receive keyboard input even after stdin was used for
// json import.
//
// this code can be removed once crossterm pr https://github.com/crossterm-rs/crossterm/pull/957 is merged or a better solution
// is implemented upstream.

use std::io::{
    self,
    Read,
};
#[cfg(target_os = "macos")]
use std::process::Command;

use crossterm::tty::IsTty;

#[cfg(target_os = "macos")]
pub fn get_controlling_terminal() -> Result<String, Box<dyn std::error::Error>> {
    let output = Command::new("tty")
        .stdin(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("Failed to run tty command: {e}"))?;

    if !output.status.success() {
        use std::fs::OpenOptions;
        let _test = OpenOptions::new()
            .read(true)
            .open("/dev/tty")
            .map_err(|_| "Cannot access /dev/tty")?;

        if let Ok(output) = Command::new("ps")
            .args(["-p", &std::process::id().to_string(), "-o", "tty="])
            .output()
            && output.status.success()
        {
            let tty_short = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !tty_short.is_empty() && tty_short != "??" {
                let tty_path = if tty_short.starts_with("tty") {
                    format!("/dev/{tty_short}")
                } else {
                    format!("/dev/tty{tty_short}")
                };

                if std::fs::File::open(&tty_path).is_ok() {
                    return Ok(tty_path);
                }
            }
        }

        return Err("Cannot determine controlling terminal".into());
    }

    let tty_path = String::from_utf8(output.stdout)?.trim().to_string();

    if !tty_path.starts_with("/dev/") {
        return Err("Invalid TTY path".into());
    }

    use std::fs::File;
    File::open(&tty_path).map_err(|e| format!("Cannot open {tty_path}: {e}"))?;

    Ok(tty_path)
}

#[cfg(target_os = "macos")]
pub fn redirect_stdin_to_tty() -> Result<(), Box<dyn std::error::Error>> {
    match get_controlling_terminal() {
        Ok(tty_path) => {
            use std::fs::OpenOptions;
            use std::os::unix::io::AsRawFd;

            let tty_file = OpenOptions::new()
                .read(true)
                .open(&tty_path)
                .map_err(|e| format!("Failed to open {tty_path}: {e}"))?;

            // redirect stdin to the controlling terminal using unsafe dup2
            // this is needed because crossterm can't handle stdin after it's been consumed
            let result = unsafe { libc::dup2(tty_file.as_raw_fd(), 0) };
            if result == -1 {
                return Err("Failed to redirect stdin to controlling terminal".into());
            }

            Ok(())
        }
        Err(_) => Err("Cannot determine controlling terminal".into()),
    }
}

pub fn read_stdin_content() -> Result<String, String> {
    if io::stdin().is_tty() {
        return Err("Cannot read from stdin when it's connected to a terminal. Use --json for inline JSON or pipe data to stdin.".to_string());
    }

    let mut stdin_content = String::new();
    io::stdin()
        .read_to_string(&mut stdin_content)
        .map_err(|e| format!("Failed to read from stdin: {e}"))?;

    Ok(stdin_content)
}
