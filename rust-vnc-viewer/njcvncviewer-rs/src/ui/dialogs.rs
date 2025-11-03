use crate::app::VncViewerApp;
use egui::Context;

pub fn render_all(app: &mut VncViewerApp, ctx: &Context) {
    // Options/Preferences dialog
    if app.ui_state().show_options_dialog {
        render_options_dialog(app, ctx);
    }

    // Help dialog
    if app.ui_state().show_help {
        render_help_dialog(app, ctx);
    }

    // About dialog
    if app.ui_state().show_about {
        render_about_dialog(app, ctx);
    }
}

fn render_options_dialog(app: &mut VncViewerApp, ctx: &Context) {
    let mut show = app.ui_state().show_options_dialog;
    let mut should_close = false;

    egui::Window::new("Preferences")
        .open(&mut show)
        .resizable(true)
        .default_width(400.0)
        .show(ctx, |ui| {
            ui.heading("Connection Settings");
            ui.separator();

            ui.horizontal(|ui| {
                ui.label("Max retries:");
                ui.add(egui::DragValue::new(&mut 3u32).clamp_range(0..=10));
            });

            ui.horizontal(|ui| {
                ui.label("Retry delay (ms):");
                ui.add(egui::DragValue::new(&mut 1000u64).clamp_range(100..=10000));
            });

            ui.checkbox(&mut true, "Verify TLS certificates");
            ui.checkbox(&mut false, "Allow self-signed certificates");

            ui.add_space(10.0);
            ui.heading("Display Settings");
            ui.separator();

            ui.horizontal(|ui| {
                ui.label("Default scale mode:");
                egui::ComboBox::from_label("")
                    .selected_text("Fit Window")
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut "fit", "fit", "Fit Window");
                        ui.selectable_value(&mut "fit", "native", "Native (1:1)");
                        ui.selectable_value(&mut "fit", "fill", "Fill Window");
                    });
            });

            ui.horizontal(|ui| {
                ui.label("Cursor mode:");
                egui::ComboBox::from_label("")
                    .selected_text("Local")
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut "local", "local", "Local");
                        ui.selectable_value(&mut "local", "remote", "Remote");
                        ui.selectable_value(&mut "local", "dot", "Dot");
                    });
            });

            ui.checkbox(&mut false, "Start in fullscreen");
            ui.checkbox(&mut true, "Dark mode");

            ui.add_space(10.0);
            ui.heading("Input Settings");
            ui.separator();

            ui.checkbox(&mut true, "Middle button emulation");

            ui.horizontal(|ui| {
                ui.label("Mouse throttle (ms):");
                ui.add(egui::DragValue::new(&mut 16u64).clamp_range(1..=100));
            });

            ui.horizontal(|ui| {
                ui.label("Key repeat throttle (ms):");
                ui.add(egui::DragValue::new(&mut 50u64).clamp_range(10..=200));
            });

            ui.checkbox(&mut true, "Enable trackpad gestures");

            ui.add_space(20.0);

            ui.horizontal(|ui| {
                if ui.button("OK").clicked() {
                    should_close = true;
                }
                if ui.button("Cancel").clicked() {
                    should_close = true;
                }
                if ui.button("Apply").clicked() {
                    // Apply changes without closing
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Reset to Defaults").clicked() {
                        // Reset all settings
                    }
                });
            });
        });

    if should_close || !show {
        app.ui_state_mut().show_options_dialog = false;
    }
}

fn render_help_dialog(app: &mut VncViewerApp, ctx: &Context) {
    let mut show = app.ui_state().show_help;
    let mut should_close = false;

    egui::Window::new("Keyboard Shortcuts")
        .open(&mut show)
        .resizable(true)
        .default_width(500.0)
        .default_height(400.0)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.heading("Connection");
                ui.separator();
                shortcut_row(ui, "Ctrl+N", "New connection");
                shortcut_row(ui, "Ctrl+D", "Disconnect");
                shortcut_row(ui, "Ctrl+Q", "Quit");

                ui.add_space(10.0);
                ui.heading("View");
                ui.separator();
                shortcut_row(ui, "F11, Alt+Enter", "Toggle fullscreen");
                shortcut_row(ui, "Ctrl+0", "Reset zoom");
                shortcut_row(ui, "Ctrl++", "Zoom in");
                shortcut_row(ui, "Ctrl+-", "Zoom out");
                shortcut_row(ui, "Ctrl+1", "Native scale (1:1)");
                shortcut_row(ui, "Ctrl+2", "Fit window");
                shortcut_row(ui, "Ctrl+3", "Fill window");

                ui.add_space(10.0);
                ui.heading("Special Keys");
                ui.separator();
                shortcut_row(ui, "Ctrl+Alt+Del", "Send Ctrl+Alt+Del to server");
                shortcut_row(ui, "Ctrl+Alt+End", "Send Ctrl+Alt+End to server");

                ui.add_space(10.0);
                ui.heading("Options");
                ui.separator();
                shortcut_row(ui, "Ctrl+,", "Open preferences");
                shortcut_row(ui, "Ctrl+Shift+V", "Toggle view-only mode");
                shortcut_row(ui, "F12", "Take screenshot");

                ui.add_space(10.0);
                ui.heading("Gestures (macOS)");
                ui.separator();
                shortcut_row(ui, "Pinch", "Zoom in/out");
                shortcut_row(ui, "Two-finger scroll", "Pan viewport");
                shortcut_row(ui, "Two-finger scroll + Shift", "Scroll content");
            });

            ui.add_space(10.0);

            if ui.button("Close").clicked() {
                should_close = true;
            }
        });

    if should_close || !show {
        app.ui_state_mut().show_help = false;
    }
}

fn render_about_dialog(app: &mut VncViewerApp, ctx: &Context) {
    let mut show = app.ui_state().show_about;
    let mut should_close = false;

    egui::Window::new("About TigerVNC Rust Viewer")
        .open(&mut show)
        .resizable(false)
        .default_width(350.0)
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.heading("TigerVNC Rust Viewer");
                ui.add_space(10.0);
                ui.label(format!("Version: {}", env!("CARGO_PKG_VERSION")));
                ui.label("Built with Rust and egui");
                ui.add_space(10.0);

                ui.label("A modern VNC client implementation");
                ui.label("compatible with TigerVNC server.");
                ui.add_space(10.0);

                if ui.link("https://github.com/tigervnc/tigervnc").clicked() {
                    // Open link in browser (would need web-open crate)
                }

                ui.add_space(20.0);

                if ui.button("Close").clicked() {
                    should_close = true;
                }
            });
        });

    if should_close || !show {
        app.ui_state_mut().show_about = false;
    }
}

fn shortcut_row(ui: &mut egui::Ui, shortcut: &str, description: &str) {
    ui.horizontal(|ui| {
        ui.add_sized(
            [120.0, 20.0],
            egui::Label::new(egui::RichText::new(shortcut).monospace().strong()),
        );
        ui.label(description);
    });
}
