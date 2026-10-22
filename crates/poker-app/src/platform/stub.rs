//! Non-Windows stub for the shell integration.
//!
//! Linux/WSL is the development environment, not the acceptance platform: tray icons and
//! named-mutex single-instance locks are Windows-only features of the final product. The spike
//! window still runs on Linux; it simply reports that these features are compiled out.

use eframe::egui;

/// No-op guard returned by the stub single-instance check.
#[derive(Debug, Default)]
pub struct SingleInstanceGuard;

/// Non-Windows platforms do not enforce a single instance during the spike.
pub fn ensure_single_instance(_id: &str) -> Result<SingleInstanceGuard, String> {
    Ok(SingleInstanceGuard)
}

/// Events the tray menu can deliver to the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // Only constructed by the Windows tray event loop.
pub enum ShellEvent {
    ShowWindow,
    Quit,
}

/// Stub shell: no tray icon, no native menus.
pub struct PlatformShell {
    description: String,
}

impl PlatformShell {
    pub fn init(_icon: Option<&egui::IconData>) -> Self {
        Self {
            description:
                "非 Windows 平台：tray-icon / single-instance 为 cfg(windows) 代码，未编译"
                    .to_owned(),
        }
    }

    pub fn description(&self) -> &str {
        &self.description
    }

    pub fn poll_events(&mut self) -> Vec<ShellEvent> {
        Vec::new()
    }

    /// No tray icon on this platform, so nothing to flash.
    pub fn set_alert_icon(&mut self, _alert: bool) {}
}
