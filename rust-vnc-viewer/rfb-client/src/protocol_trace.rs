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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceMessage<'a> {
    pub direction: &'a str,
    pub name: &'a str,
    pub fields: &'a str,
}

#[allow(dead_code)]
pub fn parse_trace_message(line: &str) -> Option<TraceMessage<'_>> {
    let (direction, start) = line
        .find("OUT ")
        .map(|i| ("OUT", i + 4))
        .or_else(|| line.find("IN ").map(|i| ("IN", i + 3)))?;
    let rest = line[start..].trim();
    let mut parts = rest.splitn(2, char::is_whitespace);
    let name = parts.next()?.trim();
    if name.is_empty() {
        return None;
    }
    Some(TraceMessage {
        direction,
        name,
        fields: parts.next().unwrap_or("").trim(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const KNOWN_GOOD_CACHE_TRACE: &str =
        "2026-05-03T09:00:00Z INFO protocol_trace: OUT SetEncodings n=15\n\
2026-05-03T09:00:01Z INFO protocol_trace: IN FramebufferUpdate rects=3\n\
2026-05-03T09:00:02Z INFO protocol_trace: OUT PersistentCacheQuery count=2\n\
2026-05-03T09:00:03Z INFO protocol_trace: OUT RequestCachedData cache_id=42\n";

    #[test]
    fn parses_known_good_cache_trace_fixture() {
        let parsed: Vec<_> = KNOWN_GOOD_CACHE_TRACE
            .lines()
            .filter_map(parse_trace_message)
            .collect();
        assert_eq!(parsed.len(), 4);
        assert_eq!(
            parsed[0],
            TraceMessage {
                direction: "OUT",
                name: "SetEncodings",
                fields: "n=15",
            }
        );
        assert_eq!(parsed[2].name, "PersistentCacheQuery");
        assert_eq!(parsed[3].fields, "cache_id=42");
    }
}
