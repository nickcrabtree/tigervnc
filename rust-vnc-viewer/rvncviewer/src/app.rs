use anyhow::Result;
use eframe::{egui, App, Frame};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::{debug, info, warn};

use crate::args::Args;
use crate::vnc_connection::{VncConnection, ConnectionStatus};

use crate::ui::{
    connection_dialog::ConnectionDialog,
    desktop::DesktopWindow,
    menubar::MenuBar,
    options_dialog::OptionsDialog,
    statusbar::StatusBar,
};

#[derive(Debug, Clone, PartialEq)]
pub enum AppState {
    /// Showing connection dialog
    Connecting,
    /// Connected and showing desktop
    Connected,
    /// Disconnected with error message
    Disconnected { reason: String },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppConfig {
    pub window_width: f32,
    pub window_height: f32,
    pub fullscreen: bool,
    pub view_only: bool,
    pub scaling_mode: String,
    pub encoding_preferences: Vec<String>,
    pub recent_servers: Vec<String>,
    pub remember_password: bool,
    pub auto_reconnect: bool,
    pub reconnect_delay_ms: u64,
    pub show_statusbar: bool,
    pub show_menubar: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            window_width: 1024.0,
            window_height: 768.0,
            fullscreen: false,
            view_only: false,
            scaling_mode: "fit".to_string(),
            encoding_preferences: vec![
                "Tight".to_string(),
                "ZRLE".to_string(),
                "Hextile".to_string(),
                "Raw".to_string(),
            ],
            recent_servers: Vec::new(),
            remember_password: false,
            auto_reconnect: true,
            reconnect_delay_ms: 5000,
            show_statusbar: true,
            show_menubar: true,
        }
    }
}

pub struct VncViewerApp {
    /// Command line arguments
    args: Args,
    
    /// Application configuration
    config: AppConfig,
    
    /// Current application state
    state: AppState,
    
    /// Configuration file path
    config_path: Option<PathBuf>,
    
    /// UI Components
    connection_dialog: ConnectionDialog,
    desktop_window: DesktopWindow,
    options_dialog: OptionsDialog,
    menubar: MenuBar,
    statusbar: StatusBar,
    
    /// UI State
    show_connection_dialog: bool,
    show_options_dialog: bool,
    show_about_dialog: bool,
    
    /// Connection state
    current_server: Option<String>,
    connection_stats: ConnectionStats,
    
    /// Fullscreen state tracking
    fullscreen_pending: bool,
    
    /// VNC connection manager
    vnc_connection: VncConnection,
    
    /// Tokio runtime for async operations
    runtime: tokio::runtime::Runtime,
}

#[derive(Debug, Default, Clone)]
pub struct ConnectionStats {
    pub connected: bool,
    pub server_name: String,
    pub framebuffer_size: (u32, u32),
    pub pixel_format: String,
    pub encoding: String,
    pub fps: f32,
    pub latency_ms: u32,
    pub bandwidth_kbps: f32,
    pub updates_per_sec: f32,
}

impl VncViewerApp {
    pub fn new(args: Args) -> Result<Self> {
        info!("Initializing VNC viewer application");
        
        // Load configuration
        let config_path = args.config.clone().or_else(|| {
            directories::UserDirs::new()
                .and_then(|dirs| {
                    let home_dir = dirs.home_dir().to_path_buf();
                    Some(home_dir.join(".config/rvncviewer/config.toml"))
                })
        });
        
        let config = if let Some(ref path) = config_path {
            Self::load_config(path).unwrap_or_else(|e| {
                warn!("Failed to load config from {}: {}", path.display(), e);
                AppConfig::default()
            })
        } else {
            AppConfig::default()
        };
        
        // Override config with command line args
        let mut config = config;
        if let Some(scaling) = &args.scaling {
            config.scaling_mode = scaling.clone();
        }
        if let Some(encodings) = &args.encodings {
            config.encoding_preferences = encodings.split(',').map(|s| s.trim().to_string()).collect();
        }
        config.fullscreen = args.fullscreen || config.fullscreen;
        config.view_only = args.view_only || config.view_only;
        
        // Initialize UI components
        let connection_dialog = ConnectionDialog::new(&config);
        let desktop_window = DesktopWindow::new();
        let options_dialog = OptionsDialog::new(&config);
        let menubar = MenuBar::new();
        let statusbar = StatusBar::new();
        
        // Determine initial state
        let (state, show_connection_dialog, current_server) = if let Some(server) = args.server.clone() {
            (AppState::Connecting, false, Some(server))
        } else {
            (AppState::Connecting, true, None)
        };
        
        // Create tokio runtime for async operations
        let runtime = tokio::runtime::Runtime::new()
            .expect("Failed to create tokio runtime");
        
        Ok(Self {
            args,
            config,
            state,
            config_path,
            connection_dialog,
            desktop_window,
            options_dialog,
            menubar,
            statusbar,
            show_connection_dialog,
            show_options_dialog: false,
            show_about_dialog: false,
            current_server,
            connection_stats: ConnectionStats::default(),
            fullscreen_pending: false,
            vnc_connection: VncConnection::new(),
            runtime,
        })
    }
    
    fn load_config(path: &PathBuf) -> Result<AppConfig> {
        let content = std::fs::read_to_string(path)?;
        let config = toml::from_str(&content)?;
        info!("Loaded configuration from {}", path.display());
        Ok(config)
    }
    
    fn save_config(&self) -> Result<()> {
        if let Some(ref path) = self.config_path {
            let content = toml::to_string_pretty(&self.config)?;
            std::fs::write(path, content)?;
            debug!("Saved configuration to {}", path.display());
        }
        Ok(())
    }
    
    fn handle_menu_action(&mut self, action: crate::ui::menubar::MenuAction) {
        use crate::ui::menubar::MenuAction;
        
        debug!("Handling menu action: {:?}", action);
        
        match action {
            MenuAction::NewConnection => {
                self.show_connection_dialog = true;
                self.state = AppState::Connecting;
            }
            MenuAction::Disconnect => {
                self.vnc_connection.disconnect();
                self.disconnect("User requested disconnection".to_string());
            }
            MenuAction::Options => {
                self.show_options_dialog = true;
            }
            MenuAction::About => {
                self.show_about_dialog = true;
            }
            MenuAction::Quit => {
                std::process::exit(0);
            }
            MenuAction::ToggleFullscreen => {
                self.config.fullscreen = !self.config.fullscreen;
                self.fullscreen_pending = true;
            }
            MenuAction::ToggleViewOnly => {
                self.config.view_only = !self.config.view_only;
                // TODO: Apply view-only mode to connection
            }
            MenuAction::ScalingNative => {
                self.config.scaling_mode = "native".to_string();
            }
            MenuAction::ScalingFit => {
                self.config.scaling_mode = "fit".to_string();
            }
            MenuAction::ScalingFill => {
                self.config.scaling_mode = "fill".to_string();
            }
        }
    }
    
    fn disconnect(&mut self, reason: String) {
        info!("Disconnecting: {}", reason);
        self.state = AppState::Disconnected { reason };
        self.current_server = None;
        self.connection_stats = ConnectionStats::default();
        self.show_connection_dialog = true;
    }
    
    /// Poll VNC events and update state
    fn poll_vnc_events(&mut self, ctx: &egui::Context) {
        use rfb_client::ServerEvent;
        
        while let Some(event) = self.vnc_connection.poll_event() {
            match event {
                ServerEvent::FramebufferUpdated { damage: _ } => {
                    // Update desktop window
                    if let Some((width, height)) = self.vnc_connection.framebuffer_size() {
                        if let Some(pixels) = self.vnc_connection.framebuffer_pixels() {
                            self.desktop_window.update_framebuffer(ctx, width, height, &pixels);
                        }
                    }
                    
                    // Request repaint for next frame
                    ctx.request_repaint();
                }
                ServerEvent::Connected { width, height, name, .. } => {
                    info!("Server info: {}x{} - {}", width, height, name);
                    self.connection_stats.framebuffer_size = (width as u32, height as u32);
                    self.connection_stats.server_name = name;
                }
                ServerEvent::ConnectionClosed => {
                    warn!("Server closed connection");
                    self.disconnect("Server closed connection".to_string());
                }
                ServerEvent::Error { message } => {
                    warn!("Server error: {}", message);
                    self.disconnect(format!("Error: {}", message));
                }
                ServerEvent::Bell => {
                    // Handle bell
                    debug!("Bell alert received");
                }
                ServerEvent::ServerCutText { .. } => {
                    // Handle clipboard
                    debug!("Clipboard update received");
                }
                _ => {}
            }
        }
    }
}

impl VncViewerApp {
    fn toggle_fullscreen(&mut self) {
        debug!("Fullscreen toggle requested: {}", self.config.fullscreen);
        // Fullscreen state will be applied in the update method
    }
    
    fn apply_fullscreen_state(&mut self, ctx: &egui::Context, _frame: &mut Frame) {
        debug!("Applying fullscreen state: {}", self.config.fullscreen);
        
        // Request fullscreen mode change
        ctx.send_viewport_cmd(if self.config.fullscreen {
            egui::ViewportCommand::Fullscreen(true)
        } else {
            egui::ViewportCommand::Fullscreen(false)
        });
    }
}

impl App for VncViewerApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut Frame) {
        // Handle fullscreen toggle
        if ctx.input(|i| i.key_pressed(egui::Key::F11)) {
            self.config.fullscreen = !self.config.fullscreen;
            self.fullscreen_pending = true;
        }
        
        // Apply pending fullscreen state change
        if self.fullscreen_pending {
            self.apply_fullscreen_state(ctx, frame);
            self.fullscreen_pending = false;
        }
        
        // Poll VNC events
        self.poll_vnc_events(ctx);
        
        // Handle menu bar
        if self.config.show_menubar {
            if let Some(action) = self.menubar.show(ctx) {
                self.handle_menu_action(action);
            }
        }
        
        // Show connection dialog if needed
        if self.show_connection_dialog {
            if let Some(result) = self.connection_dialog.show(ctx, &mut self.show_connection_dialog) {
                match result {
                    Ok(connection_info) => {
                        info!("Connection attempt: {}", connection_info.server);
                        self.current_server = Some(connection_info.server.clone());
                        self.state = AppState::Connecting;
                        self.show_connection_dialog = false;
                        
                        // Add to recent servers
                        if !self.config.recent_servers.contains(&connection_info.server) {
                            self.config.recent_servers.insert(0, connection_info.server.clone());
                            self.config.recent_servers.truncate(10); // Keep last 10
                        }
                        
                        // Initiate actual VNC connection
                        let server = connection_info.server.clone();
                        let password = connection_info.password.clone();
                        let shared = connection_info.shared;
                        
                        match self.runtime.block_on(async {
                            self.vnc_connection.connect(&server, None, password, shared).await
                        }) {
                            Ok(()) => {
                                if let ConnectionStatus::Connected { width, height, server_name } = self.vnc_connection.status() {
                                    info!("Successfully connected to {}", server_name);
                                    self.state = AppState::Connected;
                                    self.connection_stats.connected = true;
                                    self.connection_stats.server_name = server_name.clone();
                                    self.connection_stats.framebuffer_size = (*width as u32, *height as u32);
                                }
                            }
                            Err(e) => {
                                warn!("Connection failed: {}", e);
                                self.state = AppState::Disconnected { reason: format!("Connection failed: {}", e) };
                            }
                        }
                    }
                    Err(e) => {
                        warn!("Connection dialog error: {}", e);
                        self.state = AppState::Disconnected { reason: e.to_string() };
                    }
                }
            }
        }
        
        // Show options dialog if needed
        if self.show_options_dialog {
            if let Some(new_config) = self.options_dialog.show(ctx, &mut self.show_options_dialog, &self.config) {
                self.config = new_config;
                if let Err(e) = self.save_config() {
                    warn!("Failed to save configuration: {}", e);
                }
            }
        }
        
        // Show about dialog if needed
        if self.show_about_dialog {
            self.show_about_dialog(ctx);
        }
        
        // Main content area
        egui::CentralPanel::default().show(ctx, |ui| {
            match &self.state {
                AppState::Connecting => {
                    if !self.show_connection_dialog {
                        ui.centered_and_justified(|ui| {
                            ui.spinner();
                            ui.label("Connecting...");
                        });
                    }
                }
                AppState::Connected => {
                    // Show desktop window
                    self.desktop_window.show(ui, &self.config, &self.connection_stats);
                }
                AppState::Disconnected { reason } => {
                    let reason_text = reason.clone();
                    let reconnect_clicked = ui.centered_and_justified(|ui| {
                        ui.label(format!("Disconnected: {}", reason_text));
                        ui.button("Reconnect").clicked()
                    }).inner;
                    
                    if reconnect_clicked {
                        self.show_connection_dialog = true;
                        self.state = AppState::Connecting;
                    }
                }
            }
        });
        
        // Show status bar
        if self.config.show_statusbar {
            self.statusbar.show(ctx, &self.connection_stats);
        }
        
        // Request repaint for animations
        ctx.request_repaint();
    }
    
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        // Save persistent state
        if let Ok(config_str) = toml::to_string(&self.config) {
            storage.set_string("config", config_str);
        }
    }
    
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        info!("Application shutting down");
        if let Err(e) = self.save_config() {
            warn!("Failed to save configuration on exit: {}", e);
        }
    }
}

impl VncViewerApp {
    fn show_about_dialog(&mut self, ctx: &egui::Context) {
        egui::Window::new("About TigerVNC Viewer")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.heading("TigerVNC Viewer (Rust)");
                    ui.add_space(10.0);
                    
                    ui.label(format!("Version: {}", env!("CARGO_PKG_VERSION")));
                    ui.label("Built with Rust and egui");
                    ui.add_space(10.0);
                    
                    ui.label("A modern, cross-platform VNC viewer implementation");
                    ui.label("Part of the TigerVNC project");
                    ui.add_space(10.0);
                    
                    ui.horizontal(|ui| {
                        if ui.button("Close").clicked() {
                            self.show_about_dialog = false;
                        }
                    });
                });
            });
    }
}