use crate::app::VncViewerApp;
use egui::{Context, Ui};
use tracing::{error, info};

#[derive(Debug, Default)]
pub struct ConnectionDialogState {
    pub server_address: String,
    pub password: String,
    pub shared: bool,
    pub remember_server: bool,
}

impl ConnectionDialogState {
    pub fn new() -> Self {
        Self::default()
    }
}

pub fn render(app: &mut VncViewerApp, ui: &mut Ui, ctx: &Context) {
    ui.centered_and_justified(|ui| {
        ui.vertical_centered(|ui| {
            ui.heading("Connect to VNC Server");
            ui.add_space(20.0);
            
            // Connection form
            egui::Grid::new("connection_grid")
                .num_columns(2)
                .spacing([10.0, 10.0])
                .show(ui, |ui| {
                    ui.label("Server:");
                    let response = ui.text_edit_singleline(&mut app.ui_state_mut().server_input);
                    if response.changed() {
                        // Clear any previous errors when user types
                        if matches!(app.state(), crate::app::AppState::Error(_)) {
                            // Don't clear error state, just let user continue typing
                        }
                    }
                    ui.end_row();
                    
                    ui.label("Password:");
                    ui.add(egui::TextEdit::singleline(&mut app.ui_state_mut().password_input)
                        .password(true));
                    ui.end_row();
                    
                    ui.label("Options:");
                    ui.vertical(|ui| {
                        ui.checkbox(&mut app.ui_state_mut().connection_dialog.shared, "Shared session");
                        ui.checkbox(&mut app.ui_state_mut().connection_dialog.remember_server, "Remember server");
                    });
                    ui.end_row();
                });
            
            ui.add_space(20.0);
            
            // Connection status
            match app.state() {
                crate::app::AppState::Error(error) => {
                    ui.colored_label(egui::Color32::RED, format!("Error: {}", error));
                    ui.add_space(10.0);
                }
                _ => {}
            }
            
            // Buttons
            ui.horizontal(|ui| {
                let connect_enabled = !app.ui_state().server_input.is_empty();
                
                if ui.add_enabled(connect_enabled, egui::Button::new("Connect")).clicked() {
                    let server = app.ui_state().server_input.clone();
                    info!("Connecting to: {}", server);
                    
                    if let Err(e) = app.connect_to(&server) {
                        error!("Failed to connect: {:#}", e);
                    }
                }
                
                if ui.button("Options").clicked() {
                    app.ui_state_mut().show_options_dialog = true;
                }
                
                if ui.button("Help").clicked() {
                    app.ui_state_mut().show_help = true;
                }
            });
            
            ui.add_space(20.0);
            
            // Recent connections (placeholder)
            ui.group(|ui| {
                ui.label("Recent Connections:");
                ui.separator();
                
                // This would be populated from config
                if let Some(ref default_server) = app.config().connection.default_server {
                    if ui.small_button(default_server).clicked() {
                        app.ui_state_mut().server_input = default_server.clone();
                    }
                } else {
                    ui.label("(no recent connections)");
                }
            });
        });
    });
}