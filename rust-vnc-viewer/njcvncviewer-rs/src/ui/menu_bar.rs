use crate::app::VncViewerApp;
use egui::{Context, ViewportCommand};

pub fn render(app: &mut VncViewerApp, ctx: &Context) {
    egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
        egui::menu::bar(ui, |ui| {
            // File menu
            ui.menu_button("File", |ui| {
                if ui.button("New Connection...").clicked() {
                    app.disconnect_from_server();
                    ui.close_menu();
                }

                ui.separator();

                if ui
                    .add_enabled(
                        matches!(app.state(), crate::app::AppState::Connected(_)),
                        egui::Button::new("Disconnect"),
                    )
                    .clicked()
                {
                    app.disconnect_from_server();
                    ui.close_menu();
                }

                ui.separator();

                if ui.button("Quit").clicked() {
                    ctx.send_viewport_cmd(ViewportCommand::Close);
                }
            });

            // View menu
            ui.menu_button("View", |ui| {
                if ui
                    .checkbox(&mut app.ui_state_mut().fullscreen, "Fullscreen")
                    .clicked()
                {
                    ctx.send_viewport_cmd(ViewportCommand::Fullscreen(app.ui_state().fullscreen));
                    ui.close_menu();
                }

                ui.separator();

                if ui.button("Zoom In").clicked() {
                    app.viewport_mut().zoom_in();
                    ui.close_menu();
                }

                if ui.button("Zoom Out").clicked() {
                    app.viewport_mut().zoom_out();
                    ui.close_menu();
                }

                if ui.button("Reset Zoom").clicked() {
                    app.viewport_mut().reset_zoom();
                    ui.close_menu();
                }

                ui.separator();

                ui.menu_button("Scaling", |ui| {
                    let mut current_mode = app.scale_mode();
                    if ui
                        .radio_value(
                            &mut current_mode,
                            rfb_display::ScaleMode::Native,
                            "Native (1:1)",
                        )
                        .clicked()
                    {
                        app.set_scale_mode(rfb_display::ScaleMode::Native);
                        ui.close_menu();
                    }
                    if ui
                        .radio_value(&mut current_mode, rfb_display::ScaleMode::Fit, "Fit Window")
                        .clicked()
                    {
                        app.set_scale_mode(rfb_display::ScaleMode::Fit);
                        ui.close_menu();
                    }
                    if ui
                        .radio_value(
                            &mut current_mode,
                            rfb_display::ScaleMode::Fill,
                            "Fill Window",
                        )
                        .clicked()
                    {
                        app.set_scale_mode(rfb_display::ScaleMode::Fill);
                        ui.close_menu();
                    }
                });

                ui.separator();

                ui.checkbox(&mut app.ui_state_mut().view_only, "View Only");
                ui.checkbox(&mut app.ui_state_mut().show_status_bar, "Show Status Bar");
            });

            // Options menu
            ui.menu_button("Options", |ui| {
                if ui.button("Preferences...").clicked() {
                    app.ui_state_mut().show_options_dialog = true;
                    ui.close_menu();
                }

                ui.separator();

                if ui.button("Send Ctrl+Alt+Del").clicked() {
                    // This would be handled by the shortcut system
                    ui.close_menu();
                }
            });

            // Help menu
            ui.menu_button("Help", |ui| {
                if ui.button("Keyboard Shortcuts").clicked() {
                    app.ui_state_mut().show_help = true;
                    ui.close_menu();
                }

                ui.separator();

                if ui.button("About").clicked() {
                    app.ui_state_mut().show_about = true;
                    ui.close_menu();
                }
            });

            // Connection status indicator
            ui.with_layout(
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| match app.state() {
                    crate::app::AppState::Connected(server_name) => {
                        ui.colored_label(
                            egui::Color32::GREEN,
                            format!("🟢 Connected to {}", server_name),
                        );
                    }
                    crate::app::AppState::Connecting => {
                        ui.colored_label(egui::Color32::YELLOW, "🟡 Connecting...");
                    }
                    crate::app::AppState::Error(_) => {
                        ui.colored_label(egui::Color32::RED, "🔴 Error");
                    }
                    crate::app::AppState::Disconnected => {
                        ui.colored_label(egui::Color32::GRAY, "⚫ Disconnected");
                    }
                },
            );
        });
    });
}
