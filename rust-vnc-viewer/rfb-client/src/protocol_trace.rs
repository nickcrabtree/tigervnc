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

#[derive(Debug, Default, PartialEq, Eq)]
pub struct TraceSummary {
    pub set_encodings: u32,
    pub framebuffer_update: u32,
    pub persistent_cache_query: u32,
    pub persistent_cache_eviction: u32,
    pub request_cached_data: u32,
    pub cache_ref: u32,
    pub cache_init: u32,
}

#[allow(dead_code)]
pub fn summarise_trace<'a, I>(lines: I) -> TraceSummary
where
    I: IntoIterator<Item = &'a str>,
{
    let mut s = TraceSummary::default();
    for line in lines {
        if let Some(msg) = parse_trace_message(line) {
            match msg.name {
                "SetEncodings" => s.set_encodings += 1,
                "FramebufferUpdate" => s.framebuffer_update += 1,
                "PersistentCacheQuery" => s.persistent_cache_query += 1,
                "PersistentCacheEviction" => s.persistent_cache_eviction += 1,
                "RequestCachedData" => s.request_cached_data += 1,
                "CacheRef" => s.cache_ref += 1,
                "CacheInit" => s.cache_init += 1,
                _ => {}
            }
        }
    }
    s
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
    #[test]
    fn summarises_no_cache_trace_without_cache_activity() {
        let trace = ["OUT SetEncodings n=6", "IN FramebufferUpdate rects=1"];
        let s = summarise_trace(trace);
        assert_eq!(
            (
                s.set_encodings,
                s.framebuffer_update,
                s.persistent_cache_query,
                s.persistent_cache_eviction,
                s.request_cached_data
            ),
            (1, 1, 0, 0, 0)
        );
    }

    #[test]
    fn summarises_persistent_cache_trace_activity() {
        let trace = [
            "OUT SetEncodings n=15",
            "OUT PersistentCacheQuery count=3",
            "OUT PersistentCacheEviction count=1",
            "OUT RequestCachedData cache_id=42",
            "IN FramebufferUpdate rects=4",
        ];
        let s = summarise_trace(trace);
        assert_eq!(
            (
                s.set_encodings,
                s.framebuffer_update,
                s.persistent_cache_query,
                s.persistent_cache_eviction,
                s.request_cached_data
            ),
            (1, 1, 1, 1, 1)
        );
    }
}

#[cfg(test)]
mod m1_cache_trace_tests {
    use super::*;

    #[test]
    fn parses_cache_ref_and_cache_init_trace_lines() {
        let trace = [
            "IN CacheRef kind=content cache_id=42 x=1 y=2 w=64 h=32 bytes=20",
            "IN CacheInit kind=content cache_id=42 encoding=ZRLE x=1 y=2 w=64 h=32 bytes=128",
            "IN CacheRef kind=persistent ref=00112233445566778899aabbccddeeff x=0 y=0 w=16 h=16 bytes=20",
            "IN CacheInit kind=persistent ref=00112233445566778899aabbccddeeff encoding=ZRLE x=0 y=0 w=16 h=16 bytes=256",
        ];
        let parsed: Vec<_> = trace
            .iter()
            .filter_map(|line| parse_trace_message(line))
            .collect();
        assert_eq!(parsed.len(), 4);
        assert_eq!(parsed[0].direction, "IN");
        assert_eq!(parsed[0].name, "CacheRef");
        assert!(parsed[0].fields.contains("kind=content"));
        assert_eq!(parsed[1].name, "CacheInit");
        assert!(parsed[1].fields.contains("encoding=ZRLE"));
        let summary = summarise_trace(trace);
        assert_eq!(summary.cache_ref, 2);
        assert_eq!(summary.cache_init, 2);
    }
}
