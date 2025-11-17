use once_cell::sync::Lazy;
use std::sync::atomic::{AtomicBool, Ordering};

static TRACE_ENABLED: Lazy<AtomicBool> = Lazy::new(|| {
    let on = std::env::var("RUST_VNC_TRACE")
        .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE"))
        .unwrap_or(false);
    AtomicBool::new(on)
});

#[inline]
pub fn enabled() -> bool {
    TRACE_ENABLED.load(Ordering::Relaxed)
}

#[inline]
#[allow(dead_code)]
pub fn set_enabled(on: bool) {
    TRACE_ENABLED.store(on, Ordering::Relaxed)
}

#[inline]
pub fn out_msg(name: &str, fields: &str) {
    if enabled() {
        tracing::info!(target: "protocol_trace", "OUT {} {}", name, fields);
    }
}

#[inline]
pub fn in_msg(name: &str, fields: &str) {
    if enabled() {
        tracing::info!(target: "protocol_trace", "IN  {} {}", name, fields);
    }
}

#[allow(dead_code)]
pub fn hexdump(prefix: &str, data: &[u8], max: usize) {
    if !enabled() || data.is_empty() {
        return;
    }
    let max = max.min(data.len());
    let mut line = String::new();
    for (i, b) in data[..max].iter().enumerate() {
        if i % 16 == 0 {
            if !line.is_empty() {
                tracing::info!(target: "protocol_trace", "{}{}", prefix, line);
            }
            line.clear();
        }
        use std::fmt::Write as _;
        let _ = write!(line, " {:02X}", b);
    }
    if !line.is_empty() {
        tracing::info!(target: "protocol_trace", "{}{}", prefix, line);
    }
}
