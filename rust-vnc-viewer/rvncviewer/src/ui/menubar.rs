use eframe::egui;

#[derive(Debug, Clone, PartialEq)]
pub enum MenuAction {
    // File menu
    NewConnection,
    Disconnect,
    Quit,
    
    // View menu
    ToggleFullscreen,
    ToggleViewOnly,
    ScalingNative,
    ScalingFit,
    ScalingFill,
    
    // Options menu
    Options,
    
    // Help menu
    About,
}

pub struct MenuBar {
    // Menu state
    file_menu_open: bool,
    view_menu_open: bool,
    options_menu_open: bool,
    help_menu_open: bool,
}

impl MenuBar {
    pub fn new() -> Self {
        Self {
            file_menu_open: false,
            view_menu_open: false,
            options_menu_open: false,
            help_menu_open: false,
        }
    }
    
    pub fn show(&mut self, ctx: &egui::Context) -> Option<MenuAction> {
        let mut action = None;
        
        egui::TopBottomPanel::top("menubar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                // File menu
                ui.menu_button("File", |ui| {
                    if ui.button("New Connection...").clicked() {
                        action = Some(MenuAction::NewConnection);
                        ui.close_menu();
                    }
                    
                    ui.separator();
                    
                    if ui.button("Disconnect").clicked() {
                        action = Some(MenuAction::Disconnect);
                        ui.close_menu();
                    }
                    
                    ui.separator();
                    
                    if ui.button("Quit").clicked() {
                        action = Some(MenuAction::Quit);
                        ui.close_menu();
                    }
                });
                
                // View menu
                ui.menu_button("View", |ui| {
                    if ui.button("Toggle Fullscreen").clicked() {
                        action = Some(MenuAction::ToggleFullscreen);
                        ui.close_menu();
                    }
                    
                    if ui.button("Toggle View-Only Mode").clicked() {
                        action = Some(MenuAction::ToggleViewOnly);
                        ui.close_menu();
                    }
                    
                    ui.separator();
                    
                    ui.menu_button("Scaling", |ui| {
                        if ui.button("Native (1:1)").clicked() {
                            action = Some(MenuAction::ScalingNative);
                            ui.close_menu();
                        }
                        
                        if ui.button("Fit to Window").clicked() {
                            action = Some(MenuAction::ScalingFit);
                            ui.close_menu();
                        }
                        
                        if ui.button("Fill Window").clicked() {
                            action = Some(MenuAction::ScalingFill);
                            ui.close_menu();
                        }
                    });
                });
                
                // Options menu
                ui.menu_button("Options", |ui| {
                    if ui.button("Preferences...").clicked() {
                        action = Some(MenuAction::Options);
                        ui.close_menu();
                    }
                });
                
                // Help menu
                ui.menu_button("Help", |ui| {
                    if ui.button("About TigerVNC Viewer").clicked() {
                        action = Some(MenuAction::About);
                        ui.close_menu();
                    }
                });
                
                // Add some spacing to push the following items to the right
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // You could add status indicators here, like connection status
                    ui.label("Ready");
                });
            });
        });
        
        action
    }
}