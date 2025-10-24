use crate::{app::{VncViewerApp, AppState, ConnectionStats}, AppConfig};
use egui::{Context, Ui};
use std::time::Instant;

pub mod connection_dialog;
pub mod desktop;
pub mod dialogs;
pub mod menu_bar;
pub mod status_bar;

/// UI state that persists across frames
#[derive(Debug)]
pub struct UiState {
    /// Currently show menu bar
    pub show_menu_bar: bool,
    
    /// Currently show status bar
    pub show_status_bar: bool,
    
    /// Fullscreen mode active
    pub fullscreen: bool,
    
    /// View-only mode (no input sent)
    pub view_only: bool,
    
    /// Connection dialog state
    pub connection_dialog: connection_dialog::ConnectionDialogState,
    
    /// Options dialog open
    pub show_options_dialog: bool,
    
    /// Help dialog open
    pub show_help: bool,
    
    /// About dialog open
    pub show_about: bool,
    
    /// Server input field in connection dialog
    pub server_input: String,
    
    /// Password input field
    pub password_input: String,
    
    /// Last statistics update
    pub last_stats_update: Instant,
}

impl UiState {
    pub fn new() -> Self {
        Self {
            show_menu_bar: true,
            show_status_bar: true,
            fullscreen: false,
            view_only: false,
            connection_dialog: connection_dialog::ConnectionDialogState::new(),
            show_options_dialog: false,
            show_help: false,
            show_about: false,
            server_input: String::new(),
            password_input: String::new(),
            last_stats_update: Instant::now(),
        }
    }
}

/// Render the main menu bar
pub fn render_menu_bar(app: &mut VncViewerApp, ctx: &Context) {
    menu_bar::render(app, ctx);
}

/// Render the status bar
pub fn render_status_bar(app: &mut VncViewerApp, ctx: &Context) {
    status_bar::render(app, ctx);
}

/// Render the connection dialog
pub fn render_connection_dialog(app: &mut VncViewerApp, ui: &mut Ui, ctx: &Context) {
    connection_dialog::render(app, ui, ctx);
}

/// Render connecting screen
pub fn render_connecting_screen(ui: &mut Ui) {
    ui.centered_and_justified(|ui| {
        ui.vertical_centered(|ui| {
            ui.spinner();
            ui.add_space(10.0);
            ui.label("Connecting to VNC server...");
        });
    });
}

/// Render desktop area (the actual VNC display)
pub fn render_desktop_area(app: &mut VncViewerApp, ui: &mut Ui, ctx: &Context) {
    desktop::render(app, ui, ctx);
}

/// Render error screen
pub fn render_error_screen<F>(
    ui: &mut Ui, 
    error: &str, 
    on_retry: F,
    app: &mut VncViewerApp,
) 
where 
    F: FnOnce(&mut VncViewerApp)
{
    ui.centered_and_justified(|ui| {
        ui.vertical_centered(|ui| {
            ui.colored_label(egui::Color32::RED, "❌ Connection Error");
            ui.add_space(10.0);
            
            // Error message in a scrollable area for long messages
            egui::ScrollArea::vertical()
                .max_height(200.0)
                .show(ui, |ui| {
                    ui.label(error);
                });
            
            ui.add_space(20.0);
            
            ui.horizontal(|ui| {
                if ui.button("Retry Connection").clicked() {
                    on_retry(app);
                }
                
                if ui.button("New Connection").clicked() {
                    app.disconnect_from_server();
                }
            });
        });
    });
}

/// Render all modal dialogs
pub fn render_dialogs(app: &mut VncViewerApp, ctx: &Context) {
    dialogs::render_all(app, ctx);
}