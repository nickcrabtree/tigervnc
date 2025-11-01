use crate::app::VncViewerApp;
use egui::Context;
use std::time::Instant;

pub fn render(app: &mut VncViewerApp, ctx: &Context) {
    egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
        ui.horizontal(|ui| {
            // Connection info
            match app.state() {
                crate::app::AppState::Connected(server_name) => {
                    ui.label(format!("Server: {}", server_name));
                    ui.separator();

                    // Display information
                    if let Some((width, height)) = app.content_size() {
                        ui.label(format!("Size: {}x{}", width, height));
                        ui.separator();

                        let scale_mode = app.scale_mode();
                        ui.label(format!("Scale: {:?}", scale_mode));
                        ui.separator();

                        let zoom = app.zoom_factor();
                        ui.label(format!("Zoom: {:.0}%", zoom * 100.0));
                        ui.separator();
                    }

                    // Connection statistics
                    let stats = app.stats();
                    let stats_guard = stats.read();

                    ui.label(format!("Rectangles: {}", stats_guard.rectangles_received));
                    ui.separator();

                    ui.label(format!("Frames: {}", stats_guard.frames_rendered));
                    ui.separator();

                    // FPS counter
                    ui.label(format!("FPS: {:.1}", app.fps_counter()));

                    // Connection time
                    if let Some(connected_at) = stats_guard.connected_at {
                        let duration = Instant::now().duration_since(connected_at);
                        let seconds = duration.as_secs();
                        let hours = seconds / 3600;
                        let minutes = (seconds % 3600) / 60;
                        let seconds = seconds % 60;

                        ui.separator();
                        if hours > 0 {
                            ui.label(format!("Time: {}:{:02}:{:02}", hours, minutes, seconds));
                        } else {
                            ui.label(format!("Time: {}:{:02}", minutes, seconds));
                        }
                    }
                }

                crate::app::AppState::Connecting => {
                    ui.label("Connecting...");
                    ui.spinner();
                }

                crate::app::AppState::Error(error) => {
                    ui.colored_label(egui::Color32::RED, format!("Error: {}", error));
                }

                crate::app::AppState::Disconnected => {
                    ui.label("Not connected");
                }
            }

            // Right-aligned status items
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // View-only indicator
                if app.ui_state().view_only {
                    ui.colored_label(egui::Color32::from_rgb(255, 165, 0), "VIEW ONLY");
                    ui.separator();
                }

                // Fullscreen indicator
                if app.ui_state().fullscreen {
                    ui.label("FULLSCREEN");
                    ui.separator();
                }

                // Input status
                if matches!(app.state(), crate::app::AppState::Connected(_)) {
                    if app.ui_state().view_only {
                        ui.colored_label(egui::Color32::GRAY, "🚫 Input disabled");
                    } else {
                        ui.colored_label(egui::Color32::GREEN, "⌨️ Input enabled");
                    }
                }
            });
        });
    });
}
