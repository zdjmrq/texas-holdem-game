//! Platform shell integration: system tray, single-instance lock, taskbar attention.
//!
//! The Windows implementation is the only real one; on every other platform a no-op stub is
//! compiled so that Linux/WSL builds stay clean and portable. Keeping the split at module level
//! means the Linux build never sees tray-icon/single-instance code at all.

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::{PlatformShell, ShellEvent, SingleInstanceGuard, ensure_single_instance};

#[cfg(not(windows))]
mod stub;
#[cfg(not(windows))]
pub use stub::{PlatformShell, ShellEvent, SingleInstanceGuard, ensure_single_instance};
