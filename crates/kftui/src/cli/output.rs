//! Messages kftui prints outside the TUI, filtered by the same level as its
//! logs (see [`crate::cli::Cli::message_level`]). Errors and warnings go to
//! stderr. Status lines go to stdout, except when a command given after `--`
//! runs: its stdout belongs to the command, so kftui writes only to stderr.

use std::fmt::Arguments;
use std::io::Write;
use std::sync::atomic::{
    AtomicBool,
    AtomicUsize,
    Ordering,
};

use log::{
    Level,
    LevelFilter,
};

static MAX_LEVEL: AtomicUsize = AtomicUsize::new(LevelFilter::Info as usize);
static STATUS_TO_STDERR: AtomicBool = AtomicBool::new(false);

pub fn init(level: LevelFilter, status_to_stderr: bool) {
    MAX_LEVEL.store(level as usize, Ordering::Relaxed);
    STATUS_TO_STDERR.store(status_to_stderr, Ordering::Relaxed);
}

/// Write errors are dropped: a closed pipe on the reader's side must not
/// panic kftui while it still has forwards to stop.
#[doc(hidden)]
pub fn print(level: Level, args: Arguments<'_>) {
    if level as usize > MAX_LEVEL.load(Ordering::Relaxed) {
        return;
    }
    if level <= Level::Warn || STATUS_TO_STDERR.load(Ordering::Relaxed) {
        let _ = writeln!(std::io::stderr().lock(), "{args}");
    } else {
        let _ = writeln!(std::io::stdout().lock(), "{args}");
    }
}

/// Progress and results, shown at the `info` level.
macro_rules! status {
    ($($arg:tt)*) => {
        $crate::cli::output::print(::log::Level::Info, format_args!($($arg)*))
    };
}

/// Shown at the `warn` level.
macro_rules! warning {
    ($($arg:tt)*) => {
        $crate::cli::output::print(::log::Level::Warn, format_args!($($arg)*))
    };
}

/// Shown at the `error` level, the default while a command runs.
macro_rules! error {
    ($($arg:tt)*) => {
        $crate::cli::output::print(::log::Level::Error, format_args!($($arg)*))
    };
}

pub(crate) use {
    error,
    status,
    warning,
};
