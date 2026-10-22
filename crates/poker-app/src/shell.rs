//! Frameless-window helpers that work with plain egui viewport commands.
//!
//! Windows plan (see docs/spikes/egui-spike.md):
//!
//! * ViewportBuilder::with_decorations(false) + with_min_inner_size(900x600);
//! * custom title bar drawn by egui; dragging sends ViewportCommand::StartDrag;
//! * double click on the title bar toggles ViewportCommand::Maximized;
//! * edge/corner drag sends ViewportCommand::BeginResize(direction);
//! * taskbar attention uses ViewportCommand::RequestUserAttention(UserAttentionType::Critical)
//!   and Reset, which egui-winit maps to winit's request_user_attention (FlashWindowEx on
//!   Windows; X11 urgency hint elsewhere). No raw Win32 calls are needed, so the crate keeps
//!   its crate-level forbid(unsafe_code).
//!
//! All of this is testable on Linux for "does not crash"; the actual dragging/resizing feel has
//! to be verified on the Windows acceptance machine.

use eframe::egui;

/// Height of the custom title bar, in logical points.
pub const TITLEBAR_HEIGHT: f32 = 36.0;

/// Width of the invisible resize grab strips along window edges.
const RESIZE_GRAB_MARGIN: f32 = 5.0;

/// What the custom title bar wants the app to do after this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TitleBarAction {
    pub close_requested: bool,
    pub minimize_requested: bool,
    pub attention_requested: bool,
}

/// Draw the custom title bar and return the window actions requested by the user.
///
/// Must only be called when the window is undecorated (--frameless).
pub fn custom_title_bar(ui: &mut egui::Ui, title: &str) -> TitleBarAction {
    let mut action = TitleBarAction::default();
    let ctx = ui.ctx().clone();
    let bg = ui.visuals().widgets.noninteractive.bg_fill;
    let stroke = ui.visuals().widgets.noninteractive.bg_stroke;

    egui::Frame::new()
        .fill(bg)
        .stroke(stroke)
        .inner_margin(egui::Margin::symmetric(8, 4))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(title).strong().size(15.0));
                ui.add_space(6.0);

                // The remaining space is the drag handle (leave room for the buttons).
                let drag_width = (ui.available_width() - 150.0).max(0.0);
                let (drag_rect, drag_response) = ui.allocate_exact_size(
                    egui::vec2(drag_width, TITLEBAR_HEIGHT - 10.0),
                    egui::Sense::click_and_drag(),
                );
                ui.painter().text(
                    drag_rect.left_center() + egui::vec2(4.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    "（拖动此区域移动窗口；双击最大化/还原）",
                    egui::FontId::proportional(11.0),
                    ui.visuals().weak_text_color(),
                );
                if drag_response.drag_started() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
                if drag_response.double_clicked() {
                    let maximized = ctx.input(|input| input.viewport().maximized.unwrap_or(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
                }

                if ui
                    .button("—")
                    .on_hover_text("最小化")
                    .clicked()
                {
                    action.minimize_requested = true;
                }
                if ui
                    .button("▢")
                    .on_hover_text("最大化 / 还原")
                    .clicked()
                {
                    let maximized = ctx.input(|input| input.viewport().maximized.unwrap_or(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
                }
                if ui.button("✕").on_hover_text("关闭").clicked() {
                    action.close_requested = true;
                }
                if ui
                    .button("!")
                    .on_hover_text("任务栏闪烁测试 (RequestUserAttention)")
                    .clicked()
                {
                    action.attention_requested = true;
                }
            });
        });

    action
}

/// Register invisible grab strips on the 8 window edges/corners for BeginResize.
///
/// Must only be called when the window is undecorated. On Wayland this is a no-op because
/// winit does not support client-side resize there.
pub fn handle_resize_edges(ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let Some(pointer) = ctx.input(|input| input.pointer.hover_pos()) else {
        return;
    };
    let screen = ctx.content_rect();
    let margin = RESIZE_GRAB_MARGIN;
    let left = pointer.x <= screen.left() + margin;
    let right = pointer.x >= screen.right() - margin;
    let top = pointer.y <= screen.top() + margin;
    let bottom = pointer.y >= screen.bottom() - margin;

    let zone = match (top, bottom, left, right) {
        (true, false, true, false) => Some((egui::ResizeDirection::NorthWest, egui::CursorIcon::ResizeNwSe)),
        (true, false, false, true) => Some((egui::ResizeDirection::NorthEast, egui::CursorIcon::ResizeNeSw)),
        (false, true, true, false) => Some((egui::ResizeDirection::SouthWest, egui::CursorIcon::ResizeNeSw)),
        (false, true, false, true) => Some((egui::ResizeDirection::SouthEast, egui::CursorIcon::ResizeNwSe)),
        (true, false, false, false) => Some((egui::ResizeDirection::North, egui::CursorIcon::ResizeVertical)),
        (false, true, false, false) => Some((egui::ResizeDirection::South, egui::CursorIcon::ResizeVertical)),
        (false, false, true, false) => Some((egui::ResizeDirection::West, egui::CursorIcon::ResizeHorizontal)),
        (false, false, false, true) => Some((egui::ResizeDirection::East, egui::CursorIcon::ResizeHorizontal)),
        _ => None,
    };

    if let Some((direction, cursor)) = zone {
        ctx.set_cursor_icon(cursor);
        let rect = match direction {
            egui::ResizeDirection::North => egui::Rect::from_min_max(
                egui::pos2(screen.left() + margin, screen.top()),
                egui::pos2(screen.right() - margin, screen.top() + margin),
            ),
            egui::ResizeDirection::South => egui::Rect::from_min_max(
                egui::pos2(screen.left() + margin, screen.bottom() - margin),
                egui::pos2(screen.right() - margin, screen.bottom()),
            ),
            egui::ResizeDirection::West => egui::Rect::from_min_max(
                egui::pos2(screen.left(), screen.top() + margin),
                egui::pos2(screen.left() + margin, screen.bottom() - margin),
            ),
            egui::ResizeDirection::East => egui::Rect::from_min_max(
                egui::pos2(screen.right() - margin, screen.top() + margin),
                egui::pos2(screen.right(), screen.bottom() - margin),
            ),
            _ => egui::Rect::from_min_max(
                egui::pos2(pointer.x.min(screen.right() - margin), pointer.y.min(screen.bottom() - margin)),
                egui::pos2(
                    (pointer.x + margin).min(screen.right()),
                    (pointer.y + margin).min(screen.bottom()),
                ),
            ),
        };
        let response = ui.interact(
            rect,
            egui::Id::new(("poker-app-resize-edge", direction_index(direction))),
            egui::Sense::drag(),
        );
        if response.drag_started() {
            ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(direction));
        }
    }
}

fn direction_index(direction: egui::ResizeDirection) -> u8 {
    match direction {
        egui::ResizeDirection::North => 0,
        egui::ResizeDirection::South => 1,
        egui::ResizeDirection::East => 2,
        egui::ResizeDirection::West => 3,
        egui::ResizeDirection::NorthEast => 4,
        egui::ResizeDirection::SouthEast => 5,
        egui::ResizeDirection::NorthWest => 6,
        egui::ResizeDirection::SouthWest => 7,
    }
}

/// Ask the OS for user attention (Windows: FlashWindowEx via winit; X11: urgency hint).
pub fn request_attention(ctx: &egui::Context, critical: bool) {
    let attention_type = if critical {
        egui::UserAttentionType::Critical
    } else {
        egui::UserAttentionType::Informational
    };
    ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(attention_type));
}

/// Clear any pending user-attention request (stop the taskbar flash early).
pub fn clear_attention(ctx: &egui::Context) {
    ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(
        egui::UserAttentionType::Reset,
    ));
}
