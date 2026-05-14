use tracing::info;

#[derive(Debug, Clone)]
pub struct MonitorInfo {
    pub index: usize,
    pub name: String,
    pub size: (u32, u32),
    pub scale_factor: f64,
    pub is_primary: bool,
}

/// Enumerate monitors using a temporary winit EventLoop.
/// Note: Some platforms allow only one EventLoop at a time. We only create this
/// once during startup to gather metadata, and drop it immediately after.
pub fn enumerate_monitors() -> Vec<MonitorInfo> {
    let mut result = Vec::new();
    // Best-effort enumeration: API availability varies by winit version/platform
    let event_loop = winit::event_loop::EventLoop::<()>::new();

    // Try primary monitor (may be None on some platforms)
    let primary = event_loop.primary_monitor();
    let mut monitors: Vec<_> = event_loop.available_monitors().collect();
    monitors.sort_by_key(|m| {
        let size = m.size();
        (size.width, size.height)
    });

    if let Some(p) = primary {
        let pname = p.name().unwrap_or_else(|| "Primary".to_string());
        let psize = p.size();
        result.push(MonitorInfo {
            index: 0,
            name: pname.clone(),
            size: (psize.width, psize.height),
            scale_factor: p.scale_factor(),
            is_primary: true,
        });
        monitors.retain(|m| m.name() != Some(pname.clone()));
    }

    for (i, m) in monitors.into_iter().enumerate() {
        let name = m.name().unwrap_or_else(|| format!("Monitor-{}", i + 1));
        let size = m.size();
        result.push(MonitorInfo {
            index: i + 1,
            name,
            size: (size.width, size.height),
            scale_factor: m.scale_factor(),
            is_primary: false,
        });
    }

    info!("Detected {} monitor(s)", result.len());
    for m in &result {
        info!(
            "  Monitor {}: '{}' {}x{} @{}x{}",
            m.index,
            m.name,
            m.size.0,
            m.size.1,
            m.scale_factor,
            if m.is_primary { " (primary)" } else { "" }
        );
    }

    result
}

/// Select a monitor based on selector string: "primary", index, or name substring.
pub fn select_monitor(monitors: &[MonitorInfo], selector: &str) -> Option<MonitorInfo> {
    if selector.eq_ignore_ascii_case("primary") {
        return monitors
            .iter()
            .find(|m| m.is_primary)
            .cloned()
            .or_else(|| monitors.first().cloned());
    }
    if let Ok(idx) = selector.parse::<usize>() {
        return monitors.iter().find(|m| m.index == idx).cloned();
    }
    let s = selector.to_lowercase();
    monitors
        .iter()
        .find(|m| m.name.to_lowercase().contains(&s))
        .cloned()
}
