use crate::display::{select_monitor, MonitorInfo};
use tracing::{debug, info, warn};

#[derive(Debug, Clone, Default)]
pub struct FullscreenState {
    pub enabled: bool,
    pub target: Option<MonitorInfo>,
}

pub struct FullscreenController {
    state: FullscreenState,
}

impl FullscreenController {
    pub fn new() -> Self { Self { state: FullscreenState::default() } }

    pub fn state(&self) -> &FullscreenState { &self.state }

    pub fn set_target(&mut self, monitors: &[MonitorInfo], selector: Option<&str>) {
        self.state.target = selector.and_then(|s| select_monitor(monitors, s));
        if let Some(t) = &self.state.target {
            info!("Fullscreen target monitor: {} '{}', {}x{} @{}x", t.index, t.name, t.size.0, t.size.1, t.scale_factor);
        }
    }

    /// Apply fullscreen state via egui viewport command. Note: per-monitor
    /// placement is pending; current behavior uses window-manager default (usually primary).
    pub fn apply(&self, ctx: &egui::Context) {
        debug!("Applying fullscreen: {}", self.state.enabled);
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.state.enabled));
        if self.state.enabled {
            if let Some(t) = &self.state.target {
                warn!("Per-monitor fullscreen placement pending (requested '{}')", t.name);
            }
        }
    }

    pub fn toggle(&mut self) { self.state.enabled = !self.state.enabled; }
    pub fn set_enabled(&mut self, enabled: bool) { self.state.enabled = enabled; }

    /// Move to next monitor in list (cycling)
    pub fn next_monitor(&mut self, monitors: &[MonitorInfo]) {
        if monitors.is_empty() { return; }
        let current_idx = self.state.target.as_ref().map(|m| m.index).unwrap_or(0);
        let next_idx = (current_idx + 1) % monitors.len();
        self.state.target = monitors.iter().find(|m| m.index == next_idx).cloned();
        info!("Switched to monitor {}: '{}'", next_idx, self.state.target.as_ref().unwrap().name);
    }

    /// Move to previous monitor in list (cycling)
    pub fn prev_monitor(&mut self, monitors: &[MonitorInfo]) {
        if monitors.is_empty() { return; }
        let current_idx = self.state.target.as_ref().map(|m| m.index).unwrap_or(0);
        let prev_idx = if current_idx == 0 { monitors.len() - 1 } else { current_idx - 1 };
        self.state.target = monitors.iter().find(|m| m.index == prev_idx).cloned();
        info!("Switched to monitor {}: '{}'", prev_idx, self.state.target.as_ref().unwrap().name);
    }

    /// Jump to monitor by index
    pub fn jump_to_monitor(&mut self, monitors: &[MonitorInfo], index: usize) {
        if let Some(target) = monitors.iter().find(|m| m.index == index).cloned() {
            self.state.target = Some(target.clone());
            info!("Jumped to monitor {}: '{}'", index, target.name);
        } else {
            warn!("Monitor index {} not found", index);
        }
    }

    /// Jump to primary monitor
    pub fn jump_to_primary(&mut self, monitors: &[MonitorInfo]) {
        if let Some(primary) = monitors.iter().find(|m| m.is_primary).cloned() {
            self.state.target = Some(primary.clone());
            info!("Jumped to primary monitor: '{}'", primary.name);
        } else {
            warn!("Primary monitor not found");
        }
    }
}
