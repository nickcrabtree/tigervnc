use anyhow::Result;
use clap::Parser;
use eframe::egui;
use rvncviewer::app::VncViewerApp;
use rvncviewer::args::Args;
use tracing::{info, warn};

fn init_logging(verbose: bool) -> Result<()> {
    let log_level = if verbose { "debug" } else { "info" };
    
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| format!("rvncviewer={},rfb_client=info,rfb_display=info", log_level).into())
        )
        .with_target(false)
        .with_thread_ids(false)
        .with_file(false)
        .with_line_number(false)
        .init();
    
    Ok(())
}

fn create_app(args: Args) -> Result<VncViewerApp> {
    info!("Creating VNC viewer application");
    
    // Initialize configuration directory
    let config_dir = match &args.config {
        Some(path) => path.parent().map(|p| p.to_path_buf()),
        None => {
            directories::UserDirs::new()
                .and_then(|dirs| dirs.home_dir().map(|h| h.join(".config/rvncviewer")))
        }
    };
    
    if let Some(dir) = &config_dir {
        if !dir.exists() {
            std::fs::create_dir_all(dir)?;
            info!("Created config directory: {}", dir.display());
        }
    }
    
    VncViewerApp::new(args)
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    
    // Initialize logging first
    init_logging(args.verbose)?;
    
    info!("Starting rvncviewer {}", env!("CARGO_PKG_VERSION"));
    
    // Create the application
    let app = create_app(args)?;
    
    // Set up eframe options
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1024.0, 768.0])
            .with_min_inner_size([640.0, 480.0])
            .with_icon(load_icon().unwrap_or_default()),
        vsync: true,
        multisampling: 0,
        depth_buffer: 0,
        stencil_buffer: 0,
        hardware_acceleration: eframe::HardwareAcceleration::Required,
        renderer: eframe::Renderer::Glow,
        follow_system_theme: false,
        default_theme: eframe::Theme::Light,
        run_and_return: true,
        event_loop_builder: None,
        shader_version: None,
        centered: true,
        persist_window: true,
    };
    
    // Run the application
    info!("Launching GUI");
    match eframe::run_native(
        "TigerVNC Viewer",
        options,
        Box::new(move |_cc| Box::new(app)),
    ) {
        Ok(()) => {
            info!("Application exited normally");
            Ok(())
        }
        Err(e) => {
            warn!("Application exited with error: {}", e);
            Err(e.into())
        }
    }
}

fn load_icon() -> Result<egui::IconData> {
    // Try to load icon from assets directory
    let icon_path = std::env::current_exe()?
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("assets")
        .join("icon.png");
    
    if icon_path.exists() {
        let image = image::open(&icon_path)?;
        let rgba = image.to_rgba8();
        let (width, height) = rgba.dimensions();
        
        Ok(egui::IconData {
            rgba: rgba.into_raw(),
            width: width as u32,
            height: height as u32,
        })
    } else {
        // Create a simple fallback icon
        create_fallback_icon()
    }
}

fn create_fallback_icon() -> Result<egui::IconData> {
    // Create a simple 32x32 icon with a basic VNC-like design
    const SIZE: u32 = 32;
    let mut rgba = vec![0u8; (SIZE * SIZE * 4) as usize];
    
    for y in 0..SIZE {
        for x in 0..SIZE {
            let idx = ((y * SIZE + x) * 4) as usize;
            
            // Simple design: blue background with white "VNC" text area
            if x >= 4 && x <= 28 && y >= 12 && y <= 20 {
                // White text area
                rgba[idx] = 255;     // R
                rgba[idx + 1] = 255; // G
                rgba[idx + 2] = 255; // B
                rgba[idx + 3] = 255; // A
            } else {
                // Blue background
                rgba[idx] = 41;      // R
                rgba[idx + 1] = 98;  // G
                rgba[idx + 2] = 255; // B
                rgba[idx + 3] = 255; // A
            }
        }
    }
    
    Ok(egui::IconData {
        rgba,
        width: SIZE,
        height: SIZE,
    })
}