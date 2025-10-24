use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "rvncviewer")]
#[command(about = "Modern VNC viewer implementation in Rust")]
#[command(version)]
pub struct Args {
    /// VNC server address (host:display or host:port)
    pub server: Option<String>,
    
    /// Password for VNC authentication (prefer VNC_PASSWORD env var)
    #[arg(short, long, env = "VNC_PASSWORD")]
    pub password: Option<String>,
    
    /// Use view-only mode (no input sent to server)
    #[arg(long)]
    pub view_only: bool,
    
    /// Start in fullscreen mode
    #[arg(short, long)]
    pub fullscreen: bool,
    
    /// Target monitor for fullscreen: "primary", index (e.g., "1"), or name substring (e.g., "HDMI")
    #[arg(long, value_name = "SELECTOR")]
    pub monitor: Option<String>,
    
    /// Preserve aspect ratio when scaling (fit/fill)
    #[arg(long, default_value_t = true)]
    pub keep_aspect: bool,
    
    /// Configuration file path
    #[arg(short, long)]
    pub config: Option<PathBuf>,
    
    /// Enable verbose logging
    #[arg(short, long)]
    pub verbose: bool,
    
    /// Scaling mode (auto, native, fit, fill)
    #[arg(long, value_name = "MODE")]
    pub scaling: Option<String>,
    
    /// Encoding preferences (comma-separated)
    #[arg(long)]
    pub encodings: Option<String>,
}
