use eframe::egui;
use tracing::{debug, warn};

use crate::app::{AppConfig, ConnectionStats};

pub struct DesktopWindow {
    // Display state
    display_texture: Option<egui::TextureHandle>,
    framebuffer_size: (u32, u32),
    viewport_offset: egui::Vec2,
    viewport_scale: f32,
    
    // Input tracking
    mouse_pos: Option<egui::Pos2>,
    dragging: bool,
    drag_start: egui::Pos2,
    
    // Scaling and viewport
    fit_to_window: bool,
    native_scaling: bool,
    
    // Performance tracking
    frame_count: u32,
    last_fps_time: std::time::Instant,
    current_fps: f32,
}

impl DesktopWindow {
    pub fn new() -> Self {
        Self {
            display_texture: None,
            framebuffer_size: (800, 600), // Default size
            viewport_offset: egui::Vec2::ZERO,
            viewport_scale: 1.0,
            mouse_pos: None,
            dragging: false,
            drag_start: egui::Pos2::ZERO,
            fit_to_window: true,
            native_scaling: false,
            frame_count: 0,
            last_fps_time: std::time::Instant::now(),
            current_fps: 0.0,
        }
    }
    
    pub fn show(&mut self, ui: &mut egui::Ui, config: &AppConfig, stats: &ConnectionStats) {
        // Update FPS calculation
        self.update_fps();
        
        // Handle scaling mode changes
        self.update_scaling_mode(&config.scaling_mode);
        
        // Main desktop display area
        let available_size = ui.available_size();
        let response = ui.allocate_response(available_size, egui::Sense::click_and_drag());
        let rect = response.rect;
        
        // Handle input events
        self.handle_input(&response, rect, config.view_only);
        
        // Render the remote desktop
        self.render_desktop(ui, rect, stats);
        
        // Show overlay information if needed
        if ui.input(|i| i.key_pressed(egui::Key::F1)) || !stats.connected {
            self.show_info_overlay(ui, rect, stats);
        }
        
        // Handle viewport scaling and positioning
        self.update_viewport(rect);
    }
    
    fn update_fps(&mut self) {
        self.frame_count += 1;
        let now = std::time::Instant::now();
        let elapsed = now.duration_since(self.last_fps_time);
        
        if elapsed.as_secs_f32() >= 1.0 {
            self.current_fps = self.frame_count as f32 / elapsed.as_secs_f32();
            self.frame_count = 0;
            self.last_fps_time = now;
        }
    }
    
    fn update_scaling_mode(&mut self, scaling_mode: &str) {
        match scaling_mode {
            "native" => {
                self.native_scaling = true;
                self.fit_to_window = false;
                self.viewport_scale = 1.0;
            }
            "fit" => {
                self.fit_to_window = true;
                self.native_scaling = false;
            }
            "fill" => {
                self.fit_to_window = false;
                self.native_scaling = false;
                // Fill mode will be calculated in update_viewport
            }
            _ => {
                // Auto or unknown mode - default to fit
                self.fit_to_window = true;
                self.native_scaling = false;
            }
        }
    }
    
    fn handle_input(&mut self, response: &egui::Response, rect: egui::Rect, view_only: bool) {
        if view_only {
            return; // No input handling in view-only mode
        }
        
        // Mouse position tracking
        if let Some(pos) = response.hover_pos() {
            self.mouse_pos = Some(pos);
            
            // Convert to framebuffer coordinates
            if let Some(fb_pos) = self.screen_to_framebuffer(pos, rect) {
                debug!("Mouse at framebuffer position: {:?}", fb_pos);
                // TODO: Send mouse position to VNC server
            }
        }
        
        // Handle mouse clicks
        if response.clicked() {
            if let Some(pos) = response.interact_pointer_pos() {
                if let Some(fb_pos) = self.screen_to_framebuffer(pos, rect) {
                    debug!("Left click at framebuffer position: {:?}", fb_pos);
                    // TODO: Send mouse click to VNC server
                }
            }
        }
        
        // Handle right clicks
        if response.secondary_clicked() {
            if let Some(pos) = response.interact_pointer_pos() {
                if let Some(fb_pos) = self.screen_to_framebuffer(pos, rect) {
                    debug!("Right click at framebuffer position: {:?}", fb_pos);
                    // TODO: Send right mouse click to VNC server
                }
            }
        }
        
        // Handle dragging for panning
        if response.dragged() && !self.fit_to_window {
            if !self.dragging {
                self.dragging = true;
                self.drag_start = response.interact_pointer_pos().unwrap_or_default();
            } else if let Some(current_pos) = response.interact_pointer_pos() {
                let delta = current_pos - self.drag_start;
                self.viewport_offset += delta - self.drag_start;
                self.drag_start = current_pos;
            }
        } else {
            self.dragging = false;
        }
        
        // Handle scroll wheel for zooming
        let scroll_delta = response.ctx.input(|i| i.smooth_scroll_delta);
        if scroll_delta.y != 0.0 && rect.contains(response.interact_pointer_pos().unwrap_or_default()) {
            let zoom_factor = if scroll_delta.y > 0.0 { 1.1 } else { 0.9 };
            self.viewport_scale = (self.viewport_scale * zoom_factor).clamp(0.1, 10.0);
            debug!("Zoomed to scale: {:.2}", self.viewport_scale);
        }
    }
    
    fn render_desktop(&mut self, ui: &mut egui::Ui, rect: egui::Rect, stats: &ConnectionStats) {
        // Fill background
        ui.painter().rect_filled(rect, 0.0, egui::Color32::BLACK);
        
        if !stats.connected {
            // Show disconnected state
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Not connected",
                egui::FontId::proportional(24.0),
                egui::Color32::GRAY
            );
            return;
        }
        
        // Create or update display texture
        if self.display_texture.is_none() && stats.framebuffer_size.0 > 0 && stats.framebuffer_size.1 > 0 {
            self.create_display_texture(ui.ctx(), stats.framebuffer_size);
        }
        
        // Render the framebuffer texture
        if let Some(texture) = &self.display_texture {
            let fb_size = egui::Vec2::new(
                stats.framebuffer_size.0 as f32,
                stats.framebuffer_size.1 as f32
            );
            
            // Calculate display rectangle
            let display_rect = self.calculate_display_rect(rect, fb_size);
            
            // Draw the framebuffer
            ui.painter().image(
                texture.id(),
                display_rect,
                egui::Rect::from_min_size(egui::Pos2::ZERO, egui::Vec2::new(1.0, 1.0)),
                egui::Color32::WHITE
            );
            
            // Draw selection or cursor if needed
            self.draw_cursor(ui, display_rect);
        }
    }
    
    fn create_display_texture(&mut self, ctx: &egui::Context, size: (u32, u32)) {
        // Create a dummy texture with the correct size
        // In a real implementation, this would be updated with actual framebuffer data
        let width = size.0 as usize;
        let height = size.1 as usize;
        let pixels = vec![egui::Color32::from_rgb(64, 64, 64); width * height];
        
        let color_image = egui::ColorImage {
            size: [width, height],
            pixels,
        };
        
        self.display_texture = Some(ctx.load_texture(
            "desktop",
            color_image,
            egui::TextureOptions::LINEAR
        ));
        
        self.framebuffer_size = size;
        debug!("Created display texture: {}x{}", size.0, size.1);
    }
    
    fn calculate_display_rect(&self, available_rect: egui::Rect, fb_size: egui::Vec2) -> egui::Rect {
        if self.native_scaling {
            // 1:1 native scaling with panning offset
            let size = fb_size * self.viewport_scale;
            let pos = available_rect.center() - size * 0.5 + self.viewport_offset;
            egui::Rect::from_min_size(pos, size)
        } else if self.fit_to_window {
            // Fit to window while maintaining aspect ratio
            let available_size = available_rect.size();
            let scale_x = available_size.x / fb_size.x;
            let scale_y = available_size.y / fb_size.y;
            let scale = scale_x.min(scale_y); // Maintain aspect ratio
            
            let display_size = fb_size * scale;
            let pos = available_rect.center() - display_size * 0.5;
            egui::Rect::from_min_size(pos, display_size)
        } else {
            // Fill window (may distort aspect ratio)
            available_rect
        }
    }
    
    fn draw_cursor(&self, ui: &mut egui::Ui, display_rect: egui::Rect) {
        if let Some(mouse_pos) = self.mouse_pos {
            if display_rect.contains(mouse_pos) {
                // Draw a simple cursor indicator
                let cursor_size = 2.0;
                let cursor_rect = egui::Rect::from_center_size(mouse_pos, egui::Vec2::splat(cursor_size));
                ui.painter().rect_filled(cursor_rect, 0.0, egui::Color32::WHITE);
            }
        }
    }
    
    fn show_info_overlay(&self, ui: &mut egui::Ui, rect: egui::Rect, stats: &ConnectionStats) {
        let overlay_rect = egui::Rect::from_min_size(
            rect.min + egui::Vec2::new(10.0, 10.0),
            egui::Vec2::new(300.0, 200.0)
        );
        
        ui.painter().rect_filled(
            overlay_rect,
            5.0,
            egui::Color32::from_rgba_unmultiplied(0, 0, 0, 200)
        );
        
        ui.painter().rect_stroke(
            overlay_rect,
            5.0,
            egui::Stroke::new(1.0, egui::Color32::GRAY)
        );
        
        let text_pos = overlay_rect.min + egui::Vec2::new(10.0, 10.0);
        let font_id = egui::FontId::monospace(12.0);
        let line_height = 16.0;
        
        let info_lines = vec![
            format!("Server: {}", stats.server_name),
            format!("Resolution: {}x{}", stats.framebuffer_size.0, stats.framebuffer_size.1),
            format!("Encoding: {}", stats.encoding),
            format!("FPS: {:.1}", self.current_fps),
            format!("Latency: {}ms", stats.latency_ms),
            format!("Bandwidth: {:.1} KB/s", stats.bandwidth_kbps),
            format!("Scale: {:.1}x", self.viewport_scale),
        ];
        
        for (i, line) in info_lines.iter().enumerate() {
            ui.painter().text(
                text_pos + egui::Vec2::new(0.0, i as f32 * line_height),
                egui::Align2::LEFT_TOP,
                line,
                font_id.clone(),
                egui::Color32::WHITE
            );
        }
    }
    
    fn update_viewport(&mut self, _rect: egui::Rect) {
        // Clamp viewport offset to reasonable bounds
        if self.fit_to_window {
            self.viewport_offset = egui::Vec2::ZERO;
        }
    }
    
    fn screen_to_framebuffer(&self, screen_pos: egui::Pos2, display_rect: egui::Rect) -> Option<egui::Pos2> {
        // Convert screen coordinates to framebuffer coordinates
        if !display_rect.contains(screen_pos) {
            return None;
        }
        
        let fb_size = egui::Vec2::new(
            self.framebuffer_size.0 as f32,
            self.framebuffer_size.1 as f32
        );
        
        let relative_pos = screen_pos - display_rect.min;
        let display_size = display_rect.size();
        
        // Convert to normalized coordinates (0.0 to 1.0)
        let norm_x = relative_pos.x / display_size.x;
        let norm_y = relative_pos.y / display_size.y;
        
        // Convert to framebuffer coordinates
        let fb_x = norm_x * fb_size.x;
        let fb_y = norm_y * fb_size.y;
        
        // Clamp to framebuffer bounds
        if fb_x >= 0.0 && fb_x < fb_size.x && fb_y >= 0.0 && fb_y < fb_size.y {
            Some(egui::Pos2::new(fb_x, fb_y))
        } else {
            None
        }
    }
    
    /// Update the display texture with new framebuffer data
    pub fn update_framebuffer(&mut self, ctx: &egui::Context, data: &[u8], size: (u32, u32)) {
        if size != self.framebuffer_size {
            // Framebuffer size changed, recreate texture
            self.framebuffer_size = size;
            self.display_texture = None;
        }
        
        // Convert raw framebuffer data to egui::ColorImage
        let width = size.0 as usize;
        let height = size.1 as usize;
        
        if data.len() != width * height * 4 {
            warn!("Framebuffer data size mismatch: expected {}, got {}", width * height * 4, data.len());
            return;
        }
        
        let mut pixels = Vec::with_capacity(width * height);
        for chunk in data.chunks_exact(4) {
            // Assume RGBA format
            pixels.push(egui::Color32::from_rgba_premultiplied(
                chunk[0], chunk[1], chunk[2], chunk[3]
            ));
        }
        
        let color_image = egui::ColorImage {
            size: [width, height],
            pixels,
        };
        
        // Update or create the texture
        if let Some(texture) = &mut self.display_texture {
            texture.set(color_image, egui::TextureOptions::LINEAR);
        } else {
            self.display_texture = Some(ctx.load_texture(
                "desktop",
                color_image,
                egui::TextureOptions::LINEAR
            ));
        }
        
        debug!("Updated framebuffer texture: {}x{}", size.0, size.1);
    }
}
