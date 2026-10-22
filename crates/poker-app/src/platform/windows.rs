//! Windows-only shell integration for the spike: tray icon + single-instance lock.
//!
//! Windows is the acceptance platform, so this is the only real implementation of the platform
//! shell; platform/stub.rs keeps Linux/macOS builds free of tray-icon and single-instance.
//!
//! Taskbar flashing deliberately does not call into the windows crate from here: egui's
//! ViewportCommand::RequestUserAttention is forwarded by egui-winit to
//! winit::Window::request_user_attention, whose Windows backend calls FlashWindowEx with
//! FLASHW_ALL | FLASHW_TIMERNOFG (winit 0.30, platform_impl/windows/window.rs). That keeps this
//! crate under its forbid(unsafe_code) policy. If the product ever needs raw Win32 control
//! (SetOverlayIcon, jump lists, ITaskbarList3 progress), isolate it in a dedicated crate with a
//! scoped lint exception instead of weakening the app crate.
//!
//! Platform note from tray-icon 0.26: on Windows an event loop must already be running on the
//! thread that creates the TrayIcon. eframe creates the App from inside its running winit event
//! loop, so creating the tray in PlatformShell::init (App::new) is the supported place.

use eframe::egui;
use single_instance::SingleInstance;
use tray_icon::menu::{IsMenuItem, Menu, MenuEvent, MenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

/// Named-mutex single-instance guard; the lock is released when the guard is dropped.
pub struct SingleInstanceGuard {
    _inner: SingleInstance,
}

/// Acquire the app-wide single-instance lock. Returns an error when another instance runs.
pub fn ensure_single_instance(id: &str) -> Result<SingleInstanceGuard, String> {
    let instance =
        SingleInstance::new(id).map_err(|error| format!("single-instance error: {error}"))?;
    if !instance.is_single() {
        return Err("another instance is already running".to_owned());
    }
    Ok(SingleInstanceGuard { _inner: instance })
}

/// Events the tray menu can deliver to the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellEvent {
    ShowWindow,
    Quit,
}

/// Windows tray icon + context menu.
pub struct PlatformShell {
    tray: Option<TrayIcon>,
    icon_rgba: Option<(Vec<u8>, u32, u32)>,
    show_item: MenuItem,
    quit_item: MenuItem,
    description: String,
}

impl PlatformShell {
    pub fn init(icon: Option<&egui::IconData>) -> Self {
        let menu = Menu::new();
        let show_item = MenuItem::new("显示主窗口", true, None);
        let quit_item = MenuItem::new("退出", true, None);
        let menu_items: [&dyn IsMenuItem; 2] = [&show_item, &quit_item];
        if let Err(error) = menu.append_items(&menu_items) {
            log::warn!("failed to append tray menu items: {error}");
        }

        let icon_rgba = icon.map(|icon| (icon.rgba.clone(), icon.width, icon.height));
        let mut builder = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("德州扑克 Rust 重制版");
        if let Some((rgba, width, height)) = &icon_rgba {
            match Icon::from_rgba(rgba.clone(), *width, *height) {
                Ok(tray_icon) => builder = builder.with_icon(tray_icon),
                Err(error) => log::warn!("failed to build tray icon: {error}"),
            }
        }

        let (tray, description) = match builder.build() {
            Ok(tray) => (
                Some(tray),
                "tray-icon 0.26: Windows 托盘图标已创建".to_owned(),
            ),
            Err(error) => (None, format!("tray-icon 0.26: 托盘图标创建失败：{error}")),
        };
        Self {
            tray,
            icon_rgba,
            show_item,
            quit_item,
            description,
        }
    }

    pub fn description(&self) -> &str {
        &self.description
    }

    /// Drain tray menu events that arrived since the previous frame.
    pub fn poll_events(&mut self) -> Vec<ShellEvent> {
        let mut events = Vec::new();
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            let id = event.id();
            if id == self.show_item.id() {
                events.push(ShellEvent::ShowWindow);
            } else if id == self.quit_item.id() {
                events.push(ShellEvent::Quit);
            }
        }
        events
    }

    /// Swap the tray icon between the normal and a red-tinted "attention" variant.
    pub fn set_alert_icon(&mut self, alert: bool) {
        let Some(tray) = &self.tray else {
            return;
        };
        let Some((rgba, width, height)) = &self.icon_rgba else {
            return;
        };
        let pixels = if alert { tint_red(rgba) } else { rgba.clone() };
        match Icon::from_rgba(pixels, *width, *height) {
            Ok(icon) => {
                if let Err(error) = tray.set_icon(Some(icon)) {
                    log::warn!("failed to set tray icon: {error}");
                }
            }
            Err(error) => log::warn!("failed to build alert tray icon: {error}"),
        }
    }
}

/// Red-tinted variant of the tray icon, used while a "your turn" reminder flashes.
fn tint_red(rgba: &[u8]) -> Vec<u8> {
    let mut pixels = rgba.to_vec();
    for chunk in pixels.chunks_exact_mut(4) {
        chunk[0] = chunk[0].saturating_add(96);
        chunk[1] /= 3;
        chunk[2] /= 3;
    }
    pixels
}
