use anyhow::{anyhow, Result};
use eframe::egui;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::debug;

use crate::app::AppConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionInfo {
    pub server: String,
    pub password: Option<String>,
    pub view_only: bool,
    pub shared: bool,
    pub encoding_preferences: Vec<String>,
    pub quality: u8,        // 1-9 for JPEG quality
    pub compression: u8,    // 1-9 for compression level
}

impl Default for ConnectionInfo {
    fn default() -> Self {
        Self {
            server: String::new(),
            password: None,
            view_only: false,
            shared: true,
            encoding_preferences: vec![
                "Tight".to_string(),
                "ZRLE".to_string(), 
                "Hextile".to_string(),
                "Raw".to_string(),
            ],
            quality: 6,
            compression: 6,
        }
    }
}

pub struct ConnectionDialog {
    connection_info: ConnectionInfo,
    server_input: String,
    password_input: String,
    show_password: bool,
    show_advanced: bool,
    recent_servers: Vec<String>,
    selected_encoding: String,
    available_encodings: Vec<String>,
    validation_errors: HashMap<String, String>,
}

impl ConnectionDialog {
    pub fn new(config: &AppConfig) -> Self {
        let mut connection_info = ConnectionInfo::default();
        connection_info.encoding_preferences = config.encoding_preferences.clone();
        connection_info.view_only = config.view_only;
        
        Self {
            connection_info,
            server_input: String::new(),
            password_input: String::new(),
            show_password: false,
            show_advanced: false,
            recent_servers: config.recent_servers.clone(),
            selected_encoding: "Tight".to_string(),
            available_encodings: vec![
                "Raw".to_string(),
                "CopyRect".to_string(),
                "RRE".to_string(),
                "Hextile".to_string(),
                "Tight".to_string(),
                "ZRLE".to_string(),
            ],
            validation_errors: HashMap::new(),
        }
    }
    
    pub fn show(&mut self, ctx: &egui::Context, show: &mut bool) -> Option<Result<ConnectionInfo>> {
        let mut result = None;
        
        let mut should_close = false;
        egui::Window::new("Connect to VNC Server")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .open(show)
            .show(ctx, |ui| {
                ui.set_min_width(400.0);
                
                // Server input section
                ui.vertical(|ui| {
                    ui.heading("Server Connection");
                    ui.add_space(10.0);
                    
                    // Server address input
                    ui.horizontal(|ui| {
                        ui.label("Server:");
                        ui.add_space(10.0);
                        
                        let server_response = ui.add_sized(
                            [250.0, 20.0],
                            egui::TextEdit::singleline(&mut self.server_input)
                                .hint_text("hostname:display or hostname:port")
                        );
                        
                        // Focus on server input when dialog opens
                        if ui.memory(|mem| mem.everything_is_visible()) {
                            server_response.request_focus();
                        }
                    });
                    
                    // Show validation error for server
                    if let Some(error) = self.validation_errors.get("server") {
                        ui.colored_label(egui::Color32::RED, error);
                    }
                    
                    ui.add_space(5.0);
                    
                    // Recent servers dropdown
                    if !self.recent_servers.is_empty() {
                        ui.horizontal(|ui| {
                            ui.label("Recent:");
                            ui.add_space(10.0);
                            
                            egui::ComboBox::from_id_source("recent_servers")
                                .selected_text("Select recent server...")
                                .width(250.0)
                                .show_ui(ui, |ui| {
                                    for server in &self.recent_servers.clone() {
                                        if ui.selectable_value(&mut self.server_input, server.clone(), server).clicked() {
                                            debug!("Selected recent server: {}", server);
                                        }
                                    }
                                });
                        });
                        ui.add_space(5.0);
                    }
                    
                    // Password input
                    ui.horizontal(|ui| {
                        ui.label("Password:");
                        ui.add_space(10.0);
                        
                        let password_edit = if self.show_password {
                            egui::TextEdit::singleline(&mut self.password_input)
                        } else {
                            egui::TextEdit::singleline(&mut self.password_input).password(true)
                        };
                        
                        ui.add_sized([200.0, 20.0], password_edit);
                        
                        if ui.checkbox(&mut self.show_password, "Show").changed() {
                            debug!("Password visibility toggled: {}", self.show_password);
                        }
                    });
                    
                    ui.add_space(10.0);
                    
                    // Basic options
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut self.connection_info.view_only, "View only (no input)");
                        ui.add_space(20.0);
                        ui.checkbox(&mut self.connection_info.shared, "Shared session");
                    });
                    
                    ui.add_space(5.0);
                    
                    // Advanced options toggle
                    if ui.button(format!("{} Advanced Options", if self.show_advanced { "Hide" } else { "Show" })).clicked() {
                        self.show_advanced = !self.show_advanced;
                        debug!("Advanced options toggled: {}", self.show_advanced);
                    }
                    
                    if self.show_advanced {
                        ui.separator();
                        ui.add_space(5.0);
                        
                        // Encoding preferences
                        ui.horizontal(|ui| {
                            ui.label("Preferred encoding:");
                            ui.add_space(10.0);
                            
                            egui::ComboBox::from_id_source("encoding")
                                .selected_text(&self.selected_encoding)
                                .width(120.0)
                                .show_ui(ui, |ui| {
                                    for encoding in &self.available_encodings.clone() {
                                        if ui.selectable_value(&mut self.selected_encoding, encoding.clone(), encoding).clicked() {
                                            // Move selected encoding to front of preferences
                                            self.connection_info.encoding_preferences.retain(|e| e != encoding);
                                            self.connection_info.encoding_preferences.insert(0, encoding.clone());
                                            debug!("Selected encoding: {}", encoding);
                                        }
                                    }
                                });
                        });
                        
                        ui.add_space(5.0);
                        
                        // Quality and compression settings
                        ui.horizontal(|ui| {
                            ui.label("JPEG Quality:");
                            ui.add_space(5.0);
                            ui.add(egui::Slider::new(&mut self.connection_info.quality, 1..=9).text("quality"));
                            
                            ui.add_space(20.0);
                            
                            ui.label("Compression:");
                            ui.add_space(5.0);
                            ui.add(egui::Slider::new(&mut self.connection_info.compression, 1..=9).text("level"));
                        });
                    }
                    
                    ui.add_space(15.0);
                    ui.separator();
                    ui.add_space(10.0);
                    
                    // Action buttons
                    ui.horizontal(|ui| {
                        if ui.button("Connect").clicked() {
                            result = Some(self.validate_and_connect());
                            should_close = true;
                        }
                        
                        ui.add_space(10.0);
                        
                        if ui.button("Cancel").clicked() {
                            should_close = true;
                        }
                    });
                });
            });
        
        if should_close {
            *show = false;
        }
        
        result
    }
    
    fn validate_and_connect(&mut self) -> Result<ConnectionInfo> {
        self.validation_errors.clear();
        
        // Validate server address
        if self.server_input.trim().is_empty() {
            self.validation_errors.insert(
                "server".to_string(),
                "Server address is required".to_string()
            );
            return Err(anyhow!("Server address is required"));
        }
        
        // Parse and validate server address format
        let server = self.server_input.trim().to_string();
        if let Err(e) = self.parse_server_address(&server) {
            self.validation_errors.insert(
                "server".to_string(),
                format!("Invalid server address: {}", e)
            );
            return Err(anyhow!("Invalid server address: {}", e));
        }
        
        // Build connection info
        let mut info = self.connection_info.clone();
        info.server = server;
        info.password = if self.password_input.trim().is_empty() {
            None
        } else {
            Some(self.password_input.trim().to_string())
        };
        
        debug!("Connection validated: {}", info.server);
        Ok(info)
    }
    
    fn parse_server_address(&self, server: &str) -> Result<(String, u16)> {
        // Handle different server address formats:
        // - hostname:display (display 0-99, port = 5900 + display)
        // - hostname:port (port >= 100)
        // - hostname (default to port 5900, display 0)
        
        if server.is_empty() {
            return Err(anyhow!("Server address cannot be empty"));
        }
        
        let (hostname, port) = if let Some((host, port_or_display)) = server.rsplit_once(':') {
            let port_num: u16 = port_or_display.parse()
                .map_err(|_| anyhow!("Invalid port or display number: {}", port_or_display))?;
            
            let actual_port = if port_num < 100 {
                // Display number format (0-99) -> port = 5900 + display
                5900 + port_num
            } else {
                // Direct port number
                port_num
            };
            
            (host.to_string(), actual_port)
        } else {
            // No port specified, default to 5900 (display 0)
            (server.to_string(), 5900)
        };
        
        // Basic hostname validation
        if hostname.is_empty() {
            return Err(anyhow!("Hostname cannot be empty"));
        }
        
        if port == 0 || port > 65535 {
            return Err(anyhow!("Port must be between 1 and 65535"));
        }
        
        debug!("Parsed server address: {}:{}", hostname, port);
        Ok((hostname, port))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_parse_server_address() {
        let dialog = ConnectionDialog::new(&AppConfig::default());
        
        // Test display format
        assert_eq!(dialog.parse_server_address("localhost:0").unwrap(), ("localhost".to_string(), 5900));
        assert_eq!(dialog.parse_server_address("example.com:1").unwrap(), ("example.com".to_string(), 5901));
        assert_eq!(dialog.parse_server_address("host:99").unwrap(), ("host".to_string(), 5999));
        
        // Test port format
        assert_eq!(dialog.parse_server_address("localhost:5900").unwrap(), ("localhost".to_string(), 5900));
        assert_eq!(dialog.parse_server_address("example.com:5901").unwrap(), ("example.com".to_string(), 5901));
        
        // Test default port
        assert_eq!(dialog.parse_server_address("localhost").unwrap(), ("localhost".to_string(), 5900));
        
        // Test invalid addresses
        assert!(dialog.parse_server_address("").is_err());
        assert!(dialog.parse_server_address(":").is_err());
        assert!(dialog.parse_server_address(":5900").is_err());
        assert!(dialog.parse_server_address("host:abc").is_err());
        assert!(dialog.parse_server_address("host:70000").is_err());
    }
}
