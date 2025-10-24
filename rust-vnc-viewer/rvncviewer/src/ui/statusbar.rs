use eframe::egui;
use std::time::{Duration, Instant};

use crate::app::ConnectionStats;

pub struct StatusBar {
    last_update: Instant,
    show_details: bool,
}

impl StatusBar {
    pub fn new() -> Self {
        Self {
            last_update: Instant::now(),
            show_details: false,
        }
    }
    
    pub fn show(&mut self, ctx: &egui::Context, stats: &ConnectionStats) {
        egui::TopBottomPanel::bottom("statusbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                // Connection status
                if stats.connected {
                    ui.colored_label(egui::Color32::GREEN, "●");
                    ui.label(format!("Connected to {}", stats.server_name));
                } else {
                    ui.colored_label(egui::Color32::RED, "●");
                    ui.label("Not connected");
                }
                
                ui.separator();
                
                if stats.connected {
                    // Framebuffer info
                    ui.label(format!("{}×{}", stats.framebuffer_size.0, stats.framebuffer_size.1));
                    
                    ui.separator();
                    
                    // Encoding info
                    ui.label(format!("Encoding: {}", stats.encoding));
                    
                    ui.separator();
                    
                    // Performance metrics
                    if self.show_details {
                        ui.label(format!("FPS: {:.1}", stats.fps));
                        ui.separator();
                        ui.label(format!("Latency: {}ms", stats.latency_ms));
                        ui.separator();
                        ui.label(format!("Bandwidth: {:.1} KB/s", stats.bandwidth_kbps));
                        ui.separator();
                        ui.label(format!("Updates: {:.1}/s", stats.updates_per_sec));
                    } else {
                        ui.label(format!("FPS: {:.1}", stats.fps));
                    }
                }
                
                // Spacer to push the following items to the right
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Toggle details button
                    if ui.small_button(if self.show_details { "Less" } else { "More" }).clicked() {
                        self.show_details = !self.show_details;
                    }
                    
                    ui.separator();
                    
                    // Current time (optional)
                    let now = chrono::Local::now();
                    ui.label(now.format("%H:%M:%S").to_string());
                });
            });
        });
        
        self.last_update = Instant::now();
    }
    
    /// Format bandwidth for display
    fn format_bandwidth(kbps: f32) -> String {
        if kbps < 1024.0 {
            format!("{:.1} KB/s", kbps)
        } else {
            format!("{:.1} MB/s", kbps / 1024.0)
        }
    }
    
    /// Format latency for display
    fn format_latency(ms: u32) -> String {
        if ms < 1000 {
            format!("{}ms", ms)
        } else {
            format!("{:.1}s", ms as f32 / 1000.0)
        }
    }
    
    /// Get connection status color
    fn get_status_color(connected: bool, latency_ms: u32) -> egui::Color32 {
        if !connected {
            egui::Color32::RED
        } else if latency_ms > 500 {
            egui::Color32::YELLOW
        } else {
            egui::Color32::GREEN
        }
    }
}