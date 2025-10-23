use crate::connection::{ConnectionCommand, ConnectionEvent, ConnectionManager};
use crossbeam_channel::{Receiver, Sender};
use egui::{ColorImage, Context, TextureHandle, TextureOptions};
use rfb_pixelbuffer::{ManagedPixelBuffer, PixelBuffer, PixelFormat};
use rfb_protocol::connection::ConnectionState;
use std::sync::Arc;
use tracing::{debug, info};

pub struct VncViewerApp {
    // Connection channels
    command_tx: Sender<ConnectionCommand>,
    event_rx: Receiver<ConnectionEvent>,

    // Connection state
    state: ConnectionState,
    server_name: String,
    error_message: Option<String>,

    // Framebuffer
    framebuffer: Option<Arc<ManagedPixelBuffer>>,
    texture: Option<TextureHandle>,
    fb_width: u16,
    fb_height: u16,
    pixel_format: Option<PixelFormat>,

    // View state
    scale: f32,
    offset_x: f32,
    offset_y: f32,

    // Input state
    mouse_x: u16,
    mouse_y: u16,
    button_mask: u8,
}

impl VncViewerApp {
    pub fn new(_cc: &eframe::CreationContext<'_>, host: String, port: u16, shared: bool) -> Self {
        // Create channels
        let (event_tx, event_rx) = crossbeam_channel::unbounded();
        let (command_tx, command_rx) = crossbeam_channel::unbounded();

        // Spawn connection thread
        let manager = ConnectionManager::new(event_tx, command_rx, host.clone(), port, shared);

        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(manager.run());
        });

        Self {
            command_tx,
            event_rx,
            state: ConnectionState::Disconnected,
            server_name: format!("{}:{}", host, port),
            error_message: None,
            framebuffer: None,
            texture: None,
            fb_width: 0,
            fb_height: 0,
            pixel_format: None,
            scale: 1.0,
            offset_x: 0.0,
            offset_y: 0.0,
            mouse_x: 0,
            mouse_y: 0,
            button_mask: 0,
        }
    }

    fn process_events(&mut self, ctx: &Context) {
        while let Ok(event) = self.event_rx.try_recv() {
            match event {
                ConnectionEvent::StateChanged(state) => {
                    info!("Connection state: {:?}", state);
                    self.state = state;
                    ctx.request_repaint();
                }
                ConnectionEvent::Connected {
                    width,
                    height,
                    pixel_format,
                    server_name,
                } => {
                    info!(
                        "Connected: {} ({}x{}, {:?})",
                        server_name, width, height, pixel_format
                    );
                    self.fb_width = width;
                    self.fb_height = height;
                    self.pixel_format = Some(pixel_format);
                    self.server_name = server_name;
                    self.error_message = None;
                    ctx.request_repaint();
                }
                ConnectionEvent::FramebufferUpdate { buffer } => {
                    debug!("Framebuffer update received");
                    self.framebuffer = Some(buffer);
                    self.texture = None; // Force texture rebuild
                    ctx.request_repaint();
                }
                ConnectionEvent::Error(msg) => {
                    info!("Connection error: {}", msg);
                    self.error_message = Some(msg);
                    self.state = ConnectionState::Closed;
                    ctx.request_repaint();
                }
                ConnectionEvent::Disconnected => {
                    info!("Disconnected");
                    self.state = ConnectionState::Closed;
                    ctx.request_repaint();
                }
            }
        }
    }

    fn update_texture(&mut self, ctx: &Context) {
        if self.texture.is_none() {
            if let Some(ref fb) = self.framebuffer {
                let color_image = self.framebuffer_to_color_image(fb);
                let texture = ctx.load_texture("framebuffer", color_image, TextureOptions::LINEAR);
                self.texture = Some(texture);
            }
        }
    }

    fn framebuffer_to_color_image(&self, fb: &ManagedPixelBuffer) -> ColorImage {
        let (width, height) = fb.dimensions();
        let width = width as usize;
        let height = height as usize;
        let pf = fb.format();

        let mut pixels = Vec::with_capacity(width * height);

        // Convert framebuffer to RGBA
        let bytes_per_pixel = (pf.bits_per_pixel / 8) as usize;
        let data = fb.data();

        for y in 0..height {
            for x in 0..width {
                let offset = (y * width + x) * bytes_per_pixel;
                let pixel = &data[offset..offset + bytes_per_pixel];

                let (r, g, b) = if pf.true_color {
                    // Extract RGB components using shifts and masks
                    let value = match bytes_per_pixel {
                        4 => u32::from_ne_bytes([pixel[0], pixel[1], pixel[2], pixel[3]]),
                        3 => {
                            u32::from_ne_bytes([pixel[0], pixel[1], pixel[2], 0]) & 0x00FFFFFF
                        }
                        2 => u16::from_ne_bytes([pixel[0], pixel[1]]) as u32,
                        1 => pixel[0] as u32,
                        _ => 0,
                    };

                    let r_mask = pf.red_max as u32;
                    let g_mask = pf.green_max as u32;
                    let b_mask = pf.blue_max as u32;

                    let r = ((value >> pf.red_shift) & r_mask) * 255 / r_mask;
                    let g = ((value >> pf.green_shift) & g_mask) * 255 / g_mask;
                    let b = ((value >> pf.blue_shift) & b_mask) * 255 / b_mask;

                    (r as u8, g as u8, b as u8)
                } else {
                    // Color map not supported yet
                    (128, 128, 128)
                };

                pixels.push(egui::Color32::from_rgba_unmultiplied(r, g, b, 255));
            }
        }

        ColorImage {
            size: [width, height],
            pixels,
        }
    }

    fn render_status_bar(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(format!("Server: {}", self.server_name));
            ui.separator();
            ui.label(format!("State: {}", self.state));

            if let Some(ref err) = self.error_message {
                ui.separator();
                ui.colored_label(egui::Color32::RED, format!("Error: {}", err));
            }

            if self.state == ConnectionState::Normal {
                ui.separator();
                ui.label(format!("Size: {}x{}", self.fb_width, self.fb_height));
                ui.separator();
                ui.label(format!("Scale: {:.0}%", self.scale * 100.0));
            }
        });
    }

    fn render_framebuffer(&mut self, ui: &mut egui::Ui) {
        if let Some(ref texture) = self.texture {
            let _available_size = ui.available_size();

            // Calculate scaled size
            let scaled_width = self.fb_width as f32 * self.scale;
            let scaled_height = self.fb_height as f32 * self.scale;

            // Create scrollable area
            egui::ScrollArea::both()
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    let (rect, response) = ui.allocate_exact_size(
                        egui::vec2(scaled_width, scaled_height),
                        egui::Sense::click_and_drag(),
                    );

                    // Draw texture
                    ui.painter().image(
                        texture.id(),
                        rect,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );

                    // Handle mouse input
                    if response.hovered() {
                        if let Some(pos) = response.hover_pos() {
                            let rel_x = ((pos.x - rect.min.x) / self.scale) as u16;
                            let rel_y = ((pos.y - rect.min.y) / self.scale) as u16;

                            if rel_x != self.mouse_x || rel_y != self.mouse_y {
                                self.mouse_x = rel_x.min(self.fb_width - 1);
                                self.mouse_y = rel_y.min(self.fb_height - 1);

                                let _ = self.command_tx.send(ConnectionCommand::PointerEvent {
                                    button_mask: self.button_mask,
                                    x: self.mouse_x,
                                    y: self.mouse_y,
                                });
                            }
                        }
                    }

                    // Handle clicks
                    if response.clicked() {
                        self.button_mask |= 0x01; // Left button
                        let _ = self.command_tx.send(ConnectionCommand::PointerEvent {
                            button_mask: self.button_mask,
                            x: self.mouse_x,
                            y: self.mouse_y,
                        });
                    }

                    if !ui.input(|i| i.pointer.primary_down()) && self.button_mask & 0x01 != 0 {
                        self.button_mask &= !0x01; // Release left button
                        let _ = self.command_tx.send(ConnectionCommand::PointerEvent {
                            button_mask: self.button_mask,
                            x: self.mouse_x,
                            y: self.mouse_y,
                        });
                    }
                });

            // Handle keyboard input
            ui.input(|i| {
                for event in &i.events {
                    if let egui::Event::Key {
                        key,
                        pressed,
                        ..
                    } = event
                    {
                        // Convert egui key to X11 keysym (simplified)
                        if let Some(keysym) = self.egui_key_to_x11(*key) {
                            let _ = self.command_tx.send(ConnectionCommand::KeyEvent {
                                down: *pressed,
                                key: keysym,
                            });
                        }
                    }
                }
            });

            // Request continuous updates
            if self.state == ConnectionState::Normal {
                let _ = self.command_tx.send(ConnectionCommand::RequestUpdate {
                    incremental: true,
                    x: 0,
                    y: 0,
                    width: self.fb_width,
                    height: self.fb_height,
                });
            }
        } else {
            ui.centered_and_justified(|ui| {
                ui.spinner();
                ui.label("Waiting for framebuffer...");
            });
        }
    }

    fn egui_key_to_x11(&self, key: egui::Key) -> Option<u32> {
        // X11 keysym mapping (basic set)
        use egui::Key;
        Some(match key {
            Key::A => 0x0061,
            Key::B => 0x0062,
            Key::C => 0x0063,
            Key::D => 0x0064,
            Key::E => 0x0065,
            Key::F => 0x0066,
            Key::G => 0x0067,
            Key::H => 0x0068,
            Key::I => 0x0069,
            Key::J => 0x006a,
            Key::K => 0x006b,
            Key::L => 0x006c,
            Key::M => 0x006d,
            Key::N => 0x006e,
            Key::O => 0x006f,
            Key::P => 0x0070,
            Key::Q => 0x0071,
            Key::R => 0x0072,
            Key::S => 0x0073,
            Key::T => 0x0074,
            Key::U => 0x0075,
            Key::V => 0x0076,
            Key::W => 0x0077,
            Key::X => 0x0078,
            Key::Y => 0x0079,
            Key::Z => 0x007a,
            Key::Num0 => 0x0030,
            Key::Num1 => 0x0031,
            Key::Num2 => 0x0032,
            Key::Num3 => 0x0033,
            Key::Num4 => 0x0034,
            Key::Num5 => 0x0035,
            Key::Num6 => 0x0036,
            Key::Num7 => 0x0037,
            Key::Num8 => 0x0038,
            Key::Num9 => 0x0039,
            Key::Space => 0x0020,
            Key::Enter => 0xff0d,
            Key::Escape => 0xff1b,
            Key::Backspace => 0xff08,
            Key::Tab => 0xff09,
            Key::Delete => 0xffff,
            Key::ArrowLeft => 0xff51,
            Key::ArrowUp => 0xff52,
            Key::ArrowRight => 0xff53,
            Key::ArrowDown => 0xff54,
            _ => return None,
        })
    }
}

impl eframe::App for VncViewerApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        // Process connection events
        self.process_events(ctx);

        // Update texture if needed
        self.update_texture(ctx);

        // Top panel: menu and toolbar
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Disconnect").clicked() {
                        let _ = self.command_tx.send(ConnectionCommand::Disconnect);
                        ui.close_menu();
                    }
                    if ui.button("Quit").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });

                ui.menu_button("View", |ui| {
                    if ui.button("Zoom In").clicked() {
                        self.scale = (self.scale * 1.25).min(4.0);
                        ui.close_menu();
                    }
                    if ui.button("Zoom Out").clicked() {
                        self.scale = (self.scale / 1.25).max(0.25);
                        ui.close_menu();
                    }
                    if ui.button("Reset Zoom").clicked() {
                        self.scale = 1.0;
                        ui.close_menu();
                    }
                });
            });
        });

        // Bottom panel: status bar
        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            self.render_status_bar(ui);
        });

        // Central panel: framebuffer display
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.state == ConnectionState::Normal {
                self.render_framebuffer(ui);
            } else {
                ui.centered_and_justified(|ui| {
                    match self.state {
                        ConnectionState::Disconnected => ui.label("Disconnected"),
                        ConnectionState::ProtocolVersion
                        | ConnectionState::Security
                        | ConnectionState::SecurityResult
                        | ConnectionState::ClientInit
                        | ConnectionState::ServerInit => {
                            ui.spinner();
                            ui.label(format!("Connecting... ({})", self.state))
                        }
                        ConnectionState::Closed => {
                            if let Some(ref err) = self.error_message {
                                ui.colored_label(egui::Color32::RED, format!("Error: {}", err))
                            } else {
                                ui.label("Connection closed")
                            }
                        }
                        _ => ui.label(format!("State: {}", self.state)),
                    }
                });
            }
        });

        // Request repaint for continuous updates
        if self.state == ConnectionState::Normal {
            ctx.request_repaint();
        }
    }
}
