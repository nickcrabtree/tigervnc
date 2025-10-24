use eframe::egui;
use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::app::AppConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OptionsState {
    // Display settings
    scaling_mode: String,
    show_statusbar: bool,
    show_menubar: bool,
    
    // Connection settings
    auto_reconnect: bool,
    reconnect_delay_ms: u64,
    remember_password: bool,
    
    // Input settings
    view_only: bool,
    
    // Quality settings
    encoding_preferences: Vec<String>,
    
    // Window settings
    fullscreen: bool,
}

impl From<&AppConfig> for OptionsState {
    fn from(config: &AppConfig) -> Self {
        Self {
            scaling_mode: config.scaling_mode.clone(),
            show_statusbar: config.show_statusbar,
            show_menubar: config.show_menubar,
            auto_reconnect: config.auto_reconnect,
            reconnect_delay_ms: config.reconnect_delay_ms,
            remember_password: config.remember_password,
            view_only: config.view_only,
            encoding_preferences: config.encoding_preferences.clone(),
            fullscreen: config.fullscreen,
        }
    }
}

impl From<OptionsState> for AppConfig {
    fn from(state: OptionsState) -> Self {
        AppConfig {
            scaling_mode: state.scaling_mode,
            show_statusbar: state.show_statusbar,
            show_menubar: state.show_menubar,
            auto_reconnect: state.auto_reconnect,
            reconnect_delay_ms: state.reconnect_delay_ms,
            remember_password: state.remember_password,
            view_only: state.view_only,
            encoding_preferences: state.encoding_preferences,
            fullscreen: state.fullscreen,
            // Keep other fields from default
            ..Default::default()
        }
    }
}

pub struct OptionsDialog {
    state: OptionsState,
    available_encodings: Vec<String>,
    available_scaling_modes: Vec<String>,
}

impl OptionsDialog {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            state: OptionsState::from(config),
            available_encodings: vec![
                "Tight".to_string(),
                "ZRLE".to_string(),
                "Hextile".to_string(),
                "RRE".to_string(),
                "CopyRect".to_string(),
                "Raw".to_string(),
            ],
            available_scaling_modes: vec![
                "auto".to_string(),
                "native".to_string(),
                "fit".to_string(),
                "fill".to_string(),
            ],
        }
    }
    
    pub fn show(&mut self, ctx: &egui::Context, show: &mut bool, current_config: &AppConfig) -> Option<AppConfig> {
        // Reset state when dialog opens
        if *show && self.state.scaling_mode != current_config.scaling_mode {
            self.state = OptionsState::from(current_config);
        }
        
        let mut result = None;
        
        egui::Window::new("Preferences")
            .collapsible(false)
            .resizable(true)
            .default_width(500.0)
            .default_height(600.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .open(show)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.vertical(|ui| {
                        // Display settings section
                        ui.heading("Display Settings");
                        ui.separator();
                        ui.add_space(5.0);
                        
                        ui.horizontal(|ui| {
                            ui.label("Scaling mode:");
                            ui.add_space(10.0);
                            egui::ComboBox::from_id_source("scaling_mode")
                                .selected_text(&self.state.scaling_mode)
                                .width(120.0)
                                .show_ui(ui, |ui| {
                                    for mode in &self.available_scaling_modes {
                                        let display_text = match mode.as_str() {
                                            "auto" => "Auto",
                                            "native" => "Native (1:1)",
                                            "fit" => "Fit to window",
                                            "fill" => "Fill window",
                                            _ => mode,
                                        };
                                        ui.selectable_value(&mut self.state.scaling_mode, mode.clone(), display_text);
                                    }
                                });
                        });
                        
                        ui.add_space(5.0);
                        
                        ui.checkbox(&mut self.state.show_menubar, "Show menu bar");
                        ui.checkbox(&mut self.state.show_statusbar, "Show status bar");
                        ui.checkbox(&mut self.state.fullscreen, "Start in fullscreen mode");
                        
                        ui.add_space(15.0);
                        
                        // Connection settings section
                        ui.heading("Connection Settings");
                        ui.separator();
                        ui.add_space(5.0);
                        
                        ui.checkbox(&mut self.state.auto_reconnect, "Automatically reconnect on disconnection");
                        
                        if self.state.auto_reconnect {
                            ui.horizontal(|ui| {
                                ui.label("Reconnect delay:");
                                ui.add_space(10.0);
                                let mut delay_sec = (self.state.reconnect_delay_ms / 1000) as u32;
                                if ui.add(egui::Slider::new(&mut delay_sec, 1..=60).suffix(" seconds")).changed() {
                                    self.state.reconnect_delay_ms = (delay_sec as u64) * 1000;
                                }
                            });
                        }
                        
                        ui.add_space(5.0);
                        ui.checkbox(&mut self.state.remember_password, "Remember passwords");
                        
                        ui.add_space(15.0);
                        
                        // Input settings section
                        ui.heading("Input Settings");
                        ui.separator();
                        ui.add_space(5.0);
                        
                        ui.checkbox(&mut self.state.view_only, "View-only mode (disable input)");
                        
                        ui.add_space(15.0);
                        
                        // Encoding settings section
                        ui.heading("Encoding Preferences");
                        ui.separator();
                        ui.add_space(5.0);
                        
                        ui.label("Drag to reorder encoding preferences (higher = preferred):");
                        ui.add_space(5.0);
                        
                        // Simple encoding preference list (in a real implementation, this would be drag-and-drop)
                        let mut encodings_copy = self.state.encoding_preferences.clone();
                        let mut modified = false;
                        
                        for (i, encoding) in encodings_copy.iter().enumerate() {
                            ui.horizontal(|ui| {
                                ui.label(format!("{}.", i + 1));
                                ui.label(encoding);
                                
                                // Move up button
                                if i > 0 && ui.small_button("▲").clicked() {
                                    encodings_copy.swap(i, i - 1);
                                    modified = true;
                                }
                                
                                // Move down button
                                if i < encodings_copy.len() - 1 && ui.small_button("▼").clicked() {
                                    encodings_copy.swap(i, i + 1);
                                    modified = true;
                                }
                            });
                        }
                        
                        if modified {
                            self.state.encoding_preferences = encodings_copy;
                        }
                        
                        ui.add_space(20.0);
                        ui.separator();
                        ui.add_space(10.0);
                        
                        // Action buttons
                        ui.horizontal(|ui| {
                            if ui.button("Apply").clicked() {
                                let new_config = self.create_updated_config(current_config);
                                result = Some(new_config);
                                debug!("Applied preference changes");
                            }
                            
                            ui.add_space(10.0);
                            
                            if ui.button("OK").clicked() {
                                let new_config = self.create_updated_config(current_config);
                                result = Some(new_config);
                                *show = false;
                                debug!("Applied preferences and closed dialog");
                            }
                            
                            ui.add_space(10.0);
                            
                            if ui.button("Cancel").clicked() {
                                // Reset state to current config
                                self.state = OptionsState::from(current_config);
                                *show = false;
                                debug!("Cancelled preference changes");
                            }
                            
                            ui.add_space(20.0);
                            
                            if ui.button("Reset to Defaults").clicked() {
                                self.state = OptionsState::from(&AppConfig::default());
                                debug!("Reset preferences to defaults");
                            }
                        });
                    });
                });
            });
        
        result
    }
    
    fn create_updated_config(&self, current_config: &AppConfig) -> AppConfig {
        AppConfig {
            scaling_mode: self.state.scaling_mode.clone(),
            show_statusbar: self.state.show_statusbar,
            show_menubar: self.state.show_menubar,
            auto_reconnect: self.state.auto_reconnect,
            reconnect_delay_ms: self.state.reconnect_delay_ms,
            remember_password: self.state.remember_password,
            view_only: self.state.view_only,
            encoding_preferences: self.state.encoding_preferences.clone(),
            fullscreen: self.state.fullscreen,
            // Preserve other fields from current config
            window_width: current_config.window_width,
            window_height: current_config.window_height,
            recent_servers: current_config.recent_servers.clone(),
        }
    }
}
