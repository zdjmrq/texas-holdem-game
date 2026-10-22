//! Screenshot capture for the spike.
//!
//! Uses egui's async screenshot API: Context::request_screenshot sends a ViewportCommand to the
//! integration, the backend reads back the rendered frame and invokes the callback with a
//! ColorImage. The image is encoded to PNG with the image crate and the app closes itself, which
//! makes the whole "render then capture" step scriptable from the command line.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use eframe::egui;

/// One-shot screenshot request for the spike window.
pub struct ScreenshotState {
    path: PathBuf,
    trigger_frame: u64,
    requested: bool,
    done: Arc<AtomicBool>,
    result: Arc<Mutex<Option<Result<(), String>>>>,
}

impl ScreenshotState {
    pub fn new(path: PathBuf, trigger_frame: u64) -> Self {
        Self {
            path,
            trigger_frame,
            requested: false,
            done: Arc::new(AtomicBool::new(false)),
            result: Arc::new(Mutex::new(None)),
        }
    }

    /// Ask the integration for a screenshot once the given frame index is reached.
    pub fn request_if_due(&mut self, ctx: &egui::Context, frame_index: u64) {
        if self.requested || frame_index < self.trigger_frame {
            return;
        }
        self.requested = true;
        let path = self.path.clone();
        let done = Arc::clone(&self.done);
        let result = Arc::clone(&self.result);
        println!(
            "[poker-app] requesting screenshot at frame {frame_index} -> {}",
            path.display()
        );
        ctx.request_screenshot(move |image| {
            let saved = save_color_image(&image, &path);
            match &saved {
                Ok(()) => println!(
                    "SCREENSHOT_OK {} ({}x{})",
                    path.display(),
                    image.size[0],
                    image.size[1]
                ),
                Err(error) => eprintln!("SCREENSHOT_ERR {}: {error}", path.display()),
            }
            match result.lock() {
                Ok(mut slot) => *slot = Some(saved),
                Err(poisoned) => {
                    *poisoned.into_inner() =
                        Some(Err("screenshot result mutex poisoned".to_owned()));
                }
            }
            done.store(true, Ordering::SeqCst);
        });
        ctx.request_repaint();
    }

    pub fn is_done(&self) -> bool {
        self.done.load(Ordering::SeqCst)
    }

    /// Take the save result exactly once (called when the app is about to close).
    pub fn take_result(&self) -> Option<Result<(), String>> {
        self.result.lock().ok().and_then(|mut slot| slot.take())
    }
}

/// Encode a rendered egui frame as PNG (parent directories are created if needed).
pub fn save_color_image(image: &egui::ColorImage, path: &Path) -> Result<(), String> {
    let [width, height] = image.size;
    let mut rgba = Vec::with_capacity(width * height * 4);
    for pixel in &image.pixels {
        rgba.extend_from_slice(&pixel.to_array());
    }
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("create screenshot dir {}: {error}", parent.display()))?;
    }
    image::save_buffer(
        path,
        &rgba,
        width as u32,
        height as u32,
        image::ExtendedColorType::Rgba8,
    )
    .map_err(|error| format!("encode screenshot {}: {error}", path.display()))
}
