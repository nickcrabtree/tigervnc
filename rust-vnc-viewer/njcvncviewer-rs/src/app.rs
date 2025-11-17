use crate::{ui, AppConfig};
use anyhow::{Context, Result};
use arboard::Clipboard;
use egui::{Context as EguiContext, TextureHandle, ViewportCommand};
use parking_lot::RwLock;
use platform_input::*;
use rfb_client::{ClientBuilder, ClientHandle, Config, ServerEvent};
use rfb_display::{ScaleMode, Viewport, ViewportConfig};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

#[derive(Debug, Clone)]
pub enum AppState {
    /// Show connection dialog
    Connecting,
    /// VNC client connecting/connected
    Connected(String), // Server name
    /// Error state
    Error(String),
    /// Disconnected
    Disconnected,
}

/// Connection statistics
#[derive(Debug)]
pub struct ConnectionStats {
    pub connected_at: Option<Instant>,
    #[allow(dead_code)]
    pub bytes_received: u64,
    #[allow(dead_code)]
    pub bytes_sent: u64,
    pub rectangles_received: u32,
    pub frames_rendered: u32,
    pub last_update: Instant,
}

impl Default for ConnectionStats {
    fn default() -> Self {
        Self {
            connected_at: None,
            bytes_received: 0,
            bytes_sent: 0,
            rectangles_received: 0,
            frames_rendered: 0,
            last_update: Instant::now(),
        }
    }
}

use crate::display::{enumerate_monitors, MonitorInfo};
use crate::fullscreen::FullscreenController;

pub struct VncViewerApp {
    /// Application configuration
    config: AppConfig,

    /// Current application state
    state: AppState,

    /// VNC client handle (when connected)
    client_handle: Option<ClientHandle>,

    /// Channel for receiving connection results
    connection_rx: Option<mpsc::UnboundedReceiver<Result<ClientHandle, String>>>,

    /// Display renderer (placeholder)
    renderer: Option<()>,

    /// Viewport (pan/zoom/scale)
    viewport: Viewport,

    /// Current scale mode
    current_scale_mode: ScaleMode,

    /// Framebuffer dimensions (when connected)
    framebuffer_width: u32,
    framebuffer_height: u32,

    /// Input processors
    #[allow(dead_code)]
    input_dispatcher: InputDispatcher,
    #[allow(dead_code)]
    key_mapper: KeyMapper,
    #[allow(dead_code)]
    shortcuts_config: ShortcutsConfig,
    #[allow(dead_code)]
    gesture_processor: GestureProcessor,

    /// Clipboard management
    clipboard: Option<Clipboard>,
    last_clipboard_text: String,

    /// Connection statistics
    stats: Arc<RwLock<ConnectionStats>>,

    /// UI state
    ui_state: ui::UiState,

    /// Error message display
    error_message: Option<String>,

    /// Frame timing
    last_frame_time: Instant,
    frame_count: u64,
    fps_counter: f64,

    /// Framebuffer texture for rendering
    framebuffer_texture: Option<TextureHandle>,

    /// Last framebuffer update sequence number (to detect changes)
    last_framebuffer_version: u64,

    /// Fullscreen controller and monitor list
    fullscreen: FullscreenController,
    monitors: Vec<MonitorInfo>,
}

impl VncViewerApp {
    pub fn new(
        _cc: &eframe::CreationContext<'_>,
        config: AppConfig,
        initial_server: Option<String>,
        monitor_selector: Option<String>,
    ) -> Self {
        info!("Creating VNC Viewer App");

        // Set up viewport with default dimensions
        let viewport_config = ViewportConfig::default();
        let viewport = Viewport::new(viewport_config);

        // Set initial scale mode based on config
        let initial_scale_mode = match config.display.scale_mode.as_str() {
            "native" => ScaleMode::Native,
            "fit" => ScaleMode::Fit,
            "fill" => ScaleMode::Fill,
            _ => ScaleMode::Fit,
        };
        // Store scale mode in the app since Viewport doesn't have this concept directly

        // Configure input handling
        let gesture_config = GestureConfig {
            momentum_decay: config.input.scroll_momentum_decay as f64,
            zoom_sensitivity: config.input.zoom_sensitivity as f64,
            ..Default::default()
        };

        let input_dispatcher = InputDispatcher::new();
        let key_mapper = KeyMapper::new();
        let shortcuts_config = ShortcutsConfig::default();
        let gesture_processor = GestureProcessor::with_config(gesture_config);

        // Initialize clipboard (may fail on headless systems)
        let clipboard = Clipboard::new().ok();
        if clipboard.is_none() {
            warn!("Failed to initialize system clipboard - clipboard integration will be disabled");
        }

        let stats = Arc::new(RwLock::new(ConnectionStats::default()));
        let ui_state = ui::UiState::new();

        let initial_state = if initial_server.is_some() {
            AppState::Connecting
        } else {
            AppState::Disconnected
        };

        // Enumerate monitors (once at startup)
        let monitors = enumerate_monitors();
        let fullscreen = FullscreenController::new();

        let mut app = Self {
            config,
            state: initial_state,
            client_handle: None,
            connection_rx: None,
            renderer: None,
            viewport,
            current_scale_mode: initial_scale_mode,
            framebuffer_width: 0,
            framebuffer_height: 0,
            input_dispatcher,
            key_mapper,
            shortcuts_config,
            gesture_processor,
            clipboard,
            last_clipboard_text: String::new(),
            stats,
            ui_state,
            error_message: None,
            last_frame_time: Instant::now(),
            frame_count: 0,
            fps_counter: 0.0,
            framebuffer_texture: None,
            last_framebuffer_version: 0,
            fullscreen,
            monitors,
        };

        // Configure fullscreen from config and monitor selector
        app.fullscreen.set_enabled(app.config.display.fullscreen);
        if let Some(sel) = monitor_selector.as_deref() {
            app.fullscreen.set_target(&app.monitors, Some(sel));
        }

        // Auto-connect if server specified
        if let Some(server) = initial_server {
            if let Err(e) = app.connect_to_server(&server) {
                error!("Failed to auto-connect to {}: {:#}", server, e);
                app.set_error(format!("Failed to connect to {}: {:#}", server, e));
            }
        }

        app
    }

    fn connect_to_server(&mut self, server_address: &str) -> Result<()> {
        info!("Connecting to server: {}", server_address);

        let (host, port) =
            crate::parse_server_address(server_address).context("Invalid server address")?;

        // Build client configuration
        let client_config = Config::builder()
            .host(host)
            .port(port)
            .build()
            .context("Invalid client configuration")?;

        self.state = AppState::Connecting;
        self.error_message = None;

        // Reset statistics
        {
            let mut stats = self.stats.write();
            *stats = ConnectionStats::default();
            stats.last_update = Instant::now();
        }

        // Create channel for connection result
        let (tx, rx) = mpsc::unbounded_channel();
        self.connection_rx = Some(rx);

        // Spawn async connection task
        tokio::spawn(async move {
            match ClientBuilder::new(client_config).build().await {
                Ok(client) => {
                    let handle = client.handle();
                    // Send the client handle back to the main thread
                    let _ = tx.send(Ok(handle));
                    // Keep the client alive by storing it temporarily
                    // In a real implementation, we'd need to store this somewhere
                    std::mem::forget(client);
                }
                Err(e) => {
                    let _ = tx.send(Err(format!("Failed to connect: {:#}", e)));
                }
            }
        });

        Ok(())
    }

    fn disconnect(&mut self) {
        info!("Disconnecting from server");

        if let Some(handle) = self.client_handle.take() {
            if let Err(e) = handle.disconnect() {
                warn!("Error during disconnect: {:#}", e);
            }
        }

        self.renderer = None;
        self.framebuffer_texture = None;
        self.last_framebuffer_version = 0;
        self.framebuffer_width = 0;
        self.framebuffer_height = 0;
        self.state = AppState::Disconnected;
        self.error_message = None;
    }

    fn set_error<S: Into<String>>(&mut self, message: S) {
        let message = message.into();
        error!("Application error: {}", message);
        self.state = AppState::Error(message.clone());
        self.error_message = Some(message);
        self.client_handle = None;
        self.renderer = None;
    }

    fn process_connection_results(&mut self, ctx: &EguiContext) {
        if let Some(ref mut rx) = self.connection_rx {
            if let Ok(result) = rx.try_recv() {
                match result {
                    Ok(client_handle) => {
                        info!("Connection established successfully");
                        self.client_handle = Some(client_handle);
                        self.state = AppState::Connected("Connected".to_string());
                        self.connection_rx = None;
                        ctx.request_repaint();
                    }
                    Err(error) => {
                        error!("Connection failed: {}", error);
                        self.set_error(error);
                        self.connection_rx = None;
                        ctx.request_repaint();
                    }
                }
            }
        }
    }

    fn process_client_events(&mut self, ctx: &EguiContext) {
        // Clone the handle to avoid borrow conflicts
        let client_handle = self.client_handle.clone();
        if let Some(ref client_handle) = client_handle {
            while let Some(event) = client_handle.try_recv_event() {
                self.handle_client_event(event, ctx);
            }
        }
    }

    fn request_framebuffer_update(&self) {
        if let Some(ref client_handle) = self.client_handle {
            // Request incremental update for entire framebuffer using actual dimensions
            if self.framebuffer_width > 0 && self.framebuffer_height > 0 {
                let rect =
                    rfb_common::Rect::new(0, 0, self.framebuffer_width, self.framebuffer_height);
                let _ = client_handle.send(rfb_client::ClientCommand::RequestUpdate {
                    incremental: true,
                    rect: Some(rect),
                });
            }
        }
    }

    fn handle_client_event(&mut self, event: ServerEvent, ctx: &EguiContext) {
        match event {
            ServerEvent::Connected {
                width,
                height,
                name,
                ..
            } => {
                info!("Connected to server: {}", name);
                self.state = AppState::Connected(name.clone());

                // Update framebuffer dimensions
                self.framebuffer_width = width as u32;
                self.framebuffer_height = height as u32;

                // Clear any existing texture to force recreation with new size
                self.framebuffer_texture = None;
                self.last_framebuffer_version = 0;

                info!("Framebuffer dimensions: {}x{}", width, height);

                // Update stats
                {
                    let mut stats = self.stats.write();
                    stats.connected_at = Some(Instant::now());
                }

                // Note: Initial framebuffer update request is sent by event loop after SetEncodings

                ctx.request_repaint();
            }

            ServerEvent::FramebufferUpdated { damage } => {
                debug!("Framebuffer update with {} regions", damage.len());

                // Update renderer (would normally update texture/surface)
                // For now, we'll just count the update
                {
                    let mut stats = self.stats.write();
                    stats.rectangles_received += damage.len() as u32;
                    stats.last_update = Instant::now();
                }

                // Request next incremental update
                self.request_framebuffer_update();

                ctx.request_repaint();
            }

            ServerEvent::DesktopResized { width, height } => {
                info!("Desktop resized to {}x{}", width, height);
                // TODO: Update viewport content size
                ctx.request_repaint();
            }

            ServerEvent::Bell => {
                debug!("Bell received");
                // TODO: Play system bell sound
            }

            ServerEvent::ServerCutText { text } => {
                debug!("Server clipboard: {} bytes", text.len());
                // Update system clipboard with server text
                if let Ok(text_str) = String::from_utf8(text.to_vec()) {
                    if let Err(e) = self.set_clipboard_text(&text_str) {
                        warn!("Failed to set clipboard: {}", e);
                    } else {
                        info!(
                            "Updated system clipboard from server ({} bytes)",
                            text.len()
                        );
                    }
                } else {
                    warn!("Received non-UTF8 clipboard data from server");
                }
            }

            ServerEvent::Error { message } => {
                error!("Connection error: {}", message);
                self.set_error(format!("Connection error: {}", message));
                ctx.request_repaint();
            }

            ServerEvent::ConnectionClosed => {
                info!("Server disconnected");
                self.state = AppState::Disconnected;
                self.client_handle = None;
                self.renderer = None;
                ctx.request_repaint();
            }
        }
    }

    fn handle_shortcut_action(&mut self, action: ShortcutAction, ctx: &EguiContext) {
        match action {
            ShortcutAction::ToggleFullscreen => {
                ctx.send_viewport_cmd(ViewportCommand::Fullscreen(!self.ui_state.fullscreen));
                self.ui_state.fullscreen = !self.ui_state.fullscreen;
            }
            ShortcutAction::Disconnect => {
                self.disconnect();
            }
            ShortcutAction::ZoomIn => {
                self.viewport.zoom_in();
            }
            ShortcutAction::ZoomOut => {
                self.viewport.zoom_out();
            }
            ShortcutAction::ResetZoom => {
                self.viewport.reset_zoom();
            }
            ShortcutAction::ScaleNative => {
                self.set_scale_mode(ScaleMode::Native);
            }
            ShortcutAction::ScaleFit => {
                self.set_scale_mode(ScaleMode::Fit);
            }
            ShortcutAction::ScaleFill => {
                self.set_scale_mode(ScaleMode::Fill);
            }
            ShortcutAction::ToggleViewOnly => {
                self.ui_state.view_only = !self.ui_state.view_only;
                info!("View-only mode: {}", self.ui_state.view_only);
            }
            ShortcutAction::SendCtrlAltDel => {
                if let Some(ref client_handle) = self.client_handle {
                    // Send Ctrl+Alt+Del key combination
                    let _ = client_handle.send_key_event(0xFFE3, true); // Left Ctrl
                    let _ = client_handle.send_key_event(0xFFE9, true); // Left Alt
                    let _ = client_handle.send_key_event(0xFFFF, true); // Delete
                    let _ = client_handle.send_key_event(0xFFFF, false); // Delete up
                    let _ = client_handle.send_key_event(0xFFE9, false); // Left Alt up
                    let _ = client_handle.send_key_event(0xFFE3, false); // Left Ctrl up
                }
            }
            ShortcutAction::ShowHelp => {
                self.ui_state.show_help = true;
            }
            ShortcutAction::TakeScreenshot => {
                info!("Screenshot requested");
                // TODO: Implement screenshot functionality
            }
            _ => {
                debug!("Unhandled shortcut action: {:?}", action);
            }
        }
    }

    fn update_fps(&mut self) {
        let now = Instant::now();
        let frame_time = now.duration_since(self.last_frame_time).as_secs_f64();

        self.frame_count += 1;
        self.last_frame_time = now;

        // Update FPS counter with exponential smoothing
        let current_fps = 1.0 / frame_time;
        self.fps_counter = self.fps_counter * 0.9 + current_fps * 0.1;

        // Update stats
        {
            let mut stats = self.stats.write();
            stats.frames_rendered = self.frame_count as u32;
        }
    }

    /// Set system clipboard text
    fn set_clipboard_text(&mut self, text: &str) -> Result<()> {
        if let Some(ref mut clipboard) = self.clipboard {
            clipboard
                .set_text(text)
                .context("Failed to set clipboard text")?;
            self.last_clipboard_text = text.to_string();
        }
        Ok(())
    }

    /// Get system clipboard text
    fn get_clipboard_text(&mut self) -> Option<String> {
        if let Some(ref mut clipboard) = self.clipboard {
            clipboard.get_text().ok()
        } else {
            None
        }
    }

    /// Check if clipboard has changed and send to server
    fn check_and_send_clipboard(&mut self) {
        if self.ui_state.view_only {
            return;
        }

        if let Some(text) = self.get_clipboard_text() {
            // Only send if clipboard has changed
            if text != self.last_clipboard_text {
                debug!("Local clipboard changed: {} bytes", text.len());
                self.last_clipboard_text = text.clone();

                if let Some(ref client_handle) = self.client_handle {
                    if let Err(e) = client_handle.send_clipboard(bytes::Bytes::from(text)) {
                        warn!("Failed to send clipboard to server: {}", e);
                    }
                }
            }
        }
    }
}

impl eframe::App for VncViewerApp {
    fn update(&mut self, ctx: &EguiContext, frame: &mut eframe::Frame) {
        self.update_fps();

        // Process connection results from async tasks
        self.process_connection_results(ctx);

        // Process VNC client events
        self.process_client_events(ctx);

        // Check and sync clipboard when connected
        if matches!(self.state, AppState::Connected(_)) {
            self.check_and_send_clipboard();
            self.update_framebuffer_texture(ctx);
        }

        // Apply dark mode if configured
        if self.config.ui.dark_mode {
            ctx.set_visuals(egui::Visuals::dark());
        } else {
            ctx.set_visuals(egui::Visuals::light());
        }

        // Render menu bar
        if self.config.ui.show_menu_bar && self.ui_state.show_menu_bar {
            ui::render_menu_bar(self, ctx);
        }

        // Render status bar
        if self.config.ui.show_status_bar && self.ui_state.show_status_bar {
            ui::render_status_bar(self, ctx);
        }

        // Main content area
        egui::CentralPanel::default().show(ctx, |ui| match &self.state {
            AppState::Disconnected => {
                ui::render_connection_dialog(self, ui, ctx);
            }
            AppState::Connecting => {
                ui::render_connecting_screen(ui);
            }
            AppState::Connected(_server_name) => {
                ui::render_desktop_area(self, ui, ctx);
            }
            AppState::Error(error) => {
                let error_msg = error.clone();
                ui::render_error_screen(
                    ui,
                    &error_msg,
                    |app| {
                        app.state = AppState::Disconnected;
                        app.error_message = None;
                    },
                    self,
                );
            }
        });

        // Render dialogs
        ui::render_dialogs(self, ctx);

        // Handle window events and input
        self.handle_window_input(ctx, frame);

        // Request repaint if connected (for continuous updates)
        if matches!(self.state, AppState::Connected(_)) {
            ctx.request_repaint_after(Duration::from_millis(16)); // ~60 FPS
        }
    }
}

impl VncViewerApp {
    fn handle_window_input(&mut self, ctx: &EguiContext, _frame: &mut eframe::Frame) {
        // Handle keyboard shortcuts
        ctx.input(|i| {
            for event in &i.events {
                if let egui::Event::Key {
                    key,
                    pressed,
                    modifiers,
                    ..
                } = event
                {
                    if *pressed {
                        // Fullscreen shortcuts
                        let alt = modifiers.alt;
                        let ctrl = modifiers.ctrl;
                        match key {
                            egui::Key::F11 => {
                                self.fullscreen.toggle();
                                self.ui_state.fullscreen = self.fullscreen.state().enabled;
                                self.fullscreen.apply(ctx);
                                continue;
                            }
                            egui::Key::Enter if alt => {
                                self.fullscreen.toggle();
                                self.ui_state.fullscreen = self.fullscreen.state().enabled;
                                self.fullscreen.apply(ctx);
                                continue;
                            }
                            egui::Key::ArrowLeft if ctrl && alt => {
                                self.fullscreen.prev_monitor(&self.monitors);
                                self.fullscreen.apply(ctx);
                                continue;
                            }
                            egui::Key::ArrowRight if ctrl && alt => {
                                self.fullscreen.next_monitor(&self.monitors);
                                self.fullscreen.apply(ctx);
                                continue;
                            }
                            egui::Key::P if ctrl && alt => {
                                self.fullscreen.jump_to_primary(&self.monitors);
                                self.fullscreen.apply(ctx);
                                continue;
                            }
                            egui::Key::Num0
                            | egui::Key::Num1
                            | egui::Key::Num2
                            | egui::Key::Num3
                            | egui::Key::Num4
                            | egui::Key::Num5
                            | egui::Key::Num6
                            | egui::Key::Num7
                            | egui::Key::Num8
                            | egui::Key::Num9
                                if ctrl && alt =>
                            {
                                let idx = match key {
                                    egui::Key::Num0 => 0,
                                    egui::Key::Num1 => 1,
                                    egui::Key::Num2 => 2,
                                    egui::Key::Num3 => 3,
                                    egui::Key::Num4 => 4,
                                    egui::Key::Num5 => 5,
                                    egui::Key::Num6 => 6,
                                    egui::Key::Num7 => 7,
                                    egui::Key::Num8 => 8,
                                    egui::Key::Num9 => 9,
                                    _ => 0,
                                };
                                self.fullscreen.jump_to_monitor(&self.monitors, idx);
                                self.fullscreen.apply(ctx);
                                continue;
                            }
                            _ => {}
                        }

                        // Convert egui input to platform-input format
                        let active_mods = self.egui_modifiers_to_platform(*modifiers);

                        // For now, implement a simple shortcut check manually
                        if let Some(action) = self.check_egui_shortcut(*key, &active_mods) {
                            self.handle_shortcut_action(action, ctx);
                        } else if matches!(self.state, AppState::Connected(_))
                            && !self.ui_state.view_only
                        {
                            // Send regular key to VNC server
                            if let Some(keysym) = self.egui_key_to_keysym(*key) {
                                if let Some(ref client_handle) = self.client_handle {
                                    let _ = client_handle.send_key_event(keysym, *pressed);
                                }
                            }
                        }
                    }
                }
            }
        });

        // Apply fullscreen state each frame to keep window in sync
        self.fullscreen.apply(ctx);
    }

    fn check_egui_shortcut(
        &self,
        key: egui::Key,
        modifiers: &[Modifier],
    ) -> Option<ShortcutAction> {
        use egui::Key;

        // Simple shortcut mapping - in a real implementation this would use shortcuts_config
        match (key, modifiers) {
            (Key::F11, []) => Some(ShortcutAction::ToggleFullscreen),
            (Key::Escape, []) if matches!(self.state, AppState::Connected(_)) => {
                Some(ShortcutAction::ToggleFullscreen)
            }
            (Key::Q, [Modifier::Control]) => Some(ShortcutAction::Disconnect),
            (Key::F1, []) => Some(ShortcutAction::ShowHelp),
            _ => None,
        }
    }

    fn egui_modifiers_to_platform(&self, mods: egui::Modifiers) -> Vec<Modifier> {
        let mut result = Vec::new();
        if mods.shift {
            result.push(Modifier::Shift);
        }
        if mods.ctrl {
            result.push(Modifier::Control);
        }
        if mods.alt {
            result.push(Modifier::Alt);
        }
        if mods.command {
            result.push(Modifier::Super);
        }
        result
    }

    fn egui_key_to_keysym(&self, key: egui::Key) -> Option<u32> {
        // Convert egui::Key to X11 keysym
        // This is a simplified mapping - the real implementation would be in platform-input
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

    // Public getters for UI components
    pub fn config(&self) -> &AppConfig {
        &self.config
    }

    pub fn state(&self) -> &AppState {
        &self.state
    }

    pub fn stats(&self) -> Arc<RwLock<ConnectionStats>> {
        Arc::clone(&self.stats)
    }

    pub fn viewport(&self) -> &Viewport {
        &self.viewport
    }

    pub fn viewport_mut(&mut self) -> &mut Viewport {
        &mut self.viewport
    }

    /// Get viewport content size (framebuffer dimensions)
    pub fn content_size(&self) -> Option<(u32, u32)> {
        if self.framebuffer_width > 0 && self.framebuffer_height > 0 {
            Some((self.framebuffer_width, self.framebuffer_height))
        } else {
            None
        }
    }

    /// Get the current zoom factor
    pub fn zoom_factor(&self) -> f32 {
        self.viewport.zoom() as f32
    }

    /// Get the current scale mode
    pub fn scale_mode(&self) -> ScaleMode {
        self.current_scale_mode
    }

    /// Set the scale mode
    pub fn set_scale_mode(&mut self, mode: ScaleMode) {
        self.current_scale_mode = mode;
        // Apply the scale mode by adjusting viewport zoom
        // This is a simplified implementation
        match mode {
            ScaleMode::Native => {
                self.viewport.reset_zoom();
            }
            ScaleMode::Fit => {
                // Would calculate fit zoom based on window size
                // For now just use current zoom
            }
            ScaleMode::Fill => {
                // Would calculate fill zoom based on window size
                // For now just use current zoom
            }
        }
    }

    pub fn ui_state(&self) -> &ui::UiState {
        &self.ui_state
    }

    pub fn ui_state_mut(&mut self) -> &mut ui::UiState {
        &mut self.ui_state
    }

    pub fn fps_counter(&self) -> f64 {
        self.fps_counter
    }

    pub fn connect_to(&mut self, server_address: &str) -> Result<()> {
        self.connect_to_server(server_address)
    }

    pub fn disconnect_from_server(&mut self) {
        self.disconnect()
    }

    /// Update the framebuffer texture from the client handle.
    ///
    /// This should be called whenever framebuffer updates are received.
    /// Returns true if the texture was updated.
    pub fn update_framebuffer_texture(&mut self, ctx: &EguiContext) -> bool {
        let Some(ref client_handle) = self.client_handle else {
            // No client - clear texture
            if self.framebuffer_texture.is_some() {
                self.framebuffer_texture = None;
                self.last_framebuffer_version = 0;
            }
            return false;
        };

        // Get framebuffer size and format
        let (width, height) = match client_handle.framebuffer_size() {
            Some(size) => size,
            None => return false,
        };

        // Update stored dimensions
        if self.framebuffer_width != width || self.framebuffer_height != height {
            self.framebuffer_width = width;
            self.framebuffer_height = height;
            // Force texture recreation
            self.framebuffer_texture = None;
        }

        // Get framebuffer pixels
        let pixels = match client_handle.framebuffer_pixels() {
            Some(pixels) => pixels,
            None => return false,
        };

        // Simple version tracking based on content hash (for now)
        let mut hasher = DefaultHasher::new();
        pixels.hash(&mut hasher);
        let content_hash = hasher.finish();

        if content_hash == self.last_framebuffer_version && self.framebuffer_texture.is_some() {
            // No change, texture is still valid
            return false;
        }

        // Convert pixels to ColorImage
        // The framebuffer format from rfb-client should be RGB888 (3 bytes per pixel)
        let expected_len_rgb = (width * height * 3) as usize;
        let expected_len_rgba = (width * height * 4) as usize;

        let rgba_pixels =
            if pixels.len() == expected_len_rgb {
                // RGB format - convert to RGBA
                let mut rgba_pixels = Vec::with_capacity(expected_len_rgba);
                for chunk in pixels.chunks_exact(3) {
                    rgba_pixels.extend_from_slice(&[chunk[0], chunk[1], chunk[2], 255]);
                    // Add alpha
                }
                rgba_pixels
            } else if pixels.len() == expected_len_rgba {
                // Already RGBA format
                pixels
            } else {
                warn!(
                "Unexpected framebuffer pixel data length: got {}, expected {} (RGB) or {} (RGBA)", 
                pixels.len(), expected_len_rgb, expected_len_rgba
            );
                return false;
            };

        let color_image = egui::ColorImage::from_rgba_unmultiplied(
            [width as usize, height as usize],
            &rgba_pixels,
        );

        // Create or update texture
        let texture_id = format!("framebuffer_{}x{}", width, height);
        let texture = ctx.load_texture(texture_id, color_image, egui::TextureOptions::LINEAR);

        self.framebuffer_texture = Some(texture);
        self.last_framebuffer_version = content_hash;

        true
    }

    /// Get the current framebuffer texture for rendering.
    pub fn framebuffer_texture(&self) -> Option<&TextureHandle> {
        self.framebuffer_texture.as_ref()
    }
}
