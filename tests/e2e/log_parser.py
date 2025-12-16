#!/usr/bin/env python3
"""
Log parser for VNC viewer ContentCache logs.

Extracts and normalizes ContentCache operations, protocol messages,
and ARC statistics from C++ and Rust viewer logs.
"""

import re
from typing import Dict, List, Optional, Tuple
from dataclasses import dataclass, field
from pathlib import Path


@dataclass
class CacheOperation:
    """A single ContentCache operation."""
    operation: str  # 'hit', 'miss', 'store', 'lookup', 'eviction'
    cache_id: Optional[int] = None
    rect: Optional[Tuple[int, int, int, int]] = None  # x, y, w, h
    details: Optional[str] = None


@dataclass
class ProtocolMessage:
    """A ContentCache protocol message."""
    msg_type: str  # 'CachedRect', 'CachedRectInit', 'RequestCachedData'
    cache_id: Optional[int] = None
    rect: Optional[Tuple[int, int, int, int]] = None
    encoding: Optional[str] = None


@dataclass
class ARCSnapshot:
    """ARC cache state snapshot."""
    t1_size: int = 0
    t2_size: int = 0
    b1_size: int = 0
    b2_size: int = 0
    total_entries: int = 0
    memory_mb: float = 0.0
    cache_hits: int = 0
    cache_misses: int = 0
    evictions: int = 0
    
    # Client-side pixel cache ARC
    pixel_t1_size: int = 0
    pixel_t2_size: int = 0
    pixel_cache_mb: float = 0.0


@dataclass
class ParsedLog:
    """Parsed and normalized log data."""
    cache_operations: List[CacheOperation] = field(default_factory=list)
    protocol_messages: List[ProtocolMessage] = field(default_factory=list)
    arc_snapshots: List[ARCSnapshot] = field(default_factory=list)
    errors: List[str] = field(default_factory=list)
    
    # Aggregate counts (ContentCache)
    total_hits: int = 0
    total_misses: int = 0
    total_stores: int = 0
    total_lookups: int = 0
    
    # Message counts (ContentCache)
    cached_rect_count: int = 0
    cached_rect_init_count: int = 0
    request_cached_data_count: int = 0
    cache_eviction_count: int = 0  # Number of eviction notifications sent
    evicted_ids_count: int = 0  # Total number of cache IDs evicted
    
    # Negotiated cache protocol (viewer-side CConnection log)
    negotiated_contentcache: bool = False
    negotiated_persistentcache: bool = False
    
    # ContentCache bandwidth (client-side summary line)
    content_bandwidth_reduction: float = 0.0
    
    # PersistentCache-specific aggregates
    persistent_eviction_count: int = 0
    persistent_evicted_ids: int = 0
    persistent_bandwidth_reduction: float = 0.0
    persistent_hits: int = 0
    persistent_misses: int = 0
    # Number of PersistentCachedRectInit messages observed in server logs.
    # These represent initial full sends ("misses") for the PersistentCache
    # protocol, analogous to CachedRectInit for ContentCache.
    persistent_init_count: int = 0
    
    # PersistentCache initialization events (viewer-side) — should be zero when
    # PersistentCache option is disabled (e.g., PersistentCache=0).
    persistent_init_events: int = 0
    persistent_init_messages: List[str] = field(default_factory=list)
    
    # Final ARC state
    final_arc: Optional[ARCSnapshot] = None


def parse_rect_coords(text: str) -> Optional[Tuple[int, int, int, int]]:
    """
    Parse rectangle coordinates from various formats.
    
    Formats supported:
    - [x,y-x,y]
    - [x,y w×h]
    - rect=[x,y-x,y]
    """
    # Format: [x,y-x,y] or rect=[x,y-x,y]
    match = re.search(r'\[?(\d+),(\d+)-(\d+),(\d+)\]?', text)
    if match:
        x1, y1, x2, y2 = map(int, match.groups())
        return (x1, y1, x2 - x1, y2 - y1)
    
    # Format: [x,y w×h]
    match = re.search(r'\[?(\d+),(\d+)\s+(\d+)[×x](\d+)\]?', text)
    if match:
        x, y, w, h = map(int, match.groups())
        return (x, y, w, h)
    
    return None


def parse_cache_id(text: str) -> Optional[int]:
    """Extract cache ID from text."""
    # Look for cacheId=N, cache ID N, ID N, etc.
    patterns = [
        r'cacheId[=:]\s*(\d+)',
        r'cache\s+ID[=:]\s*(\d+)',
        r'\bID[=:]\s*(\d+)',
        r'for\s+ID\s+(\d+)',
    ]
    
    for pattern in patterns:
        match = re.search(pattern, text, re.IGNORECASE)
        if match:
            return int(match.group(1))
    
    return None


def parse_cpp_log(log_path: Path) -> ParsedLog:
    """Parse C++ viewer log file."""
    parsed = ParsedLog()
    
    if not log_path.exists():
        parsed.errors.append(f"Log file not found: {log_path}")
        return parsed
    
    with open(log_path, 'r', errors='replace') as f:
        for line in f:
            line = line.strip()
            
            # Skip timestamps and log prefixes (normalize)
            # Format: "2025-10-26T12:00:00.123456Z DecodeManager: message"
            if ' DecodeManager: ' in line or ' ContentCache: ' in line or ' EncodeManager: ' in line:
                # Extract just the message part
                parts = line.split(': ', 1)
                if len(parts) > 1:
                    message = parts[1]
                else:
                    message = line
            else:
                message = line
            
            lower = message.lower()
            
            # Detect PersistentCache initialization or disk-loading events in the
            # viewer log. These should *not* appear when the viewer is started
            # with PersistentCache=0.
            if (
                'persistentcache created with arc' in lower
                or 'persistentcache loaded from disk' in lower
                or 'persistentcache starting fresh' in lower
                or 'persistentcache debug log:' in lower
                # v3/v4 sharded format messages
                or 'persistentcache v' in lower
                or 'persistentcache: loading v' in lower
                or 'persistentcache: saved v' in lower
            ):
                parsed.persistent_init_events += 1
                if len(parsed.persistent_init_messages) < 10:
                    parsed.persistent_init_messages.append(message)
            
            # Cache protocol negotiation (viewer-side CConnection log)
            if 'cache protocol:' in lower and 'negotiated contentcache' in lower:
                parsed.negotiated_contentcache = True
            elif 'cache protocol:' in lower and 'negotiated persistentcache' in lower:
                parsed.negotiated_persistentcache = True
            
            # ContentCache operations
            # IMPORTANT: Be strict here — avoid matching PersistentCache lines
            # ("PersistentCache HIT") or summary/statistics lines ("Cache hits:").
            if 'contentcache' in lower and 'hit' in lower:
                cache_id = parse_cache_id(message)
                rect = parse_rect_coords(message)
                parsed.cache_operations.append(
                    CacheOperation('hit', cache_id=cache_id, rect=rect, details=message)
                )
                parsed.total_hits += 1

            elif 'contentcache' in lower and 'miss' in lower:
                cache_id = parse_cache_id(message)
                parsed.cache_operations.append(
                    CacheOperation('miss', cache_id=cache_id, details=message)
                )
                parsed.total_misses += 1

            elif ('storing decoded rect' in lower and 'contentcache' in lower) or (
                'store' in lower and 'contentcache' in lower
            ):
                cache_id = parse_cache_id(message)
                rect = parse_rect_coords(message)
                parsed.cache_operations.append(
                    CacheOperation('store', cache_id=cache_id, rect=rect, details=message)
                )
                parsed.total_stores += 1

            # Protocol messages (ContentCache only)
            # Avoid counting PersistentCachedRect* as CachedRect*.
            if 'persistentcachedrect' not in lower:
                if 'cachedrectinit' in lower:
                    cache_id = parse_cache_id(message)
                    rect = parse_rect_coords(message)
                    parsed.protocol_messages.append(
                        ProtocolMessage('CachedRectInit', cache_id=cache_id, rect=rect)
                    )
                    parsed.cached_rect_init_count += 1

                elif 'cachedrect' in lower and 'init' not in lower:
                    cache_id = parse_cache_id(message)
                    rect = parse_rect_coords(message)
                    parsed.protocol_messages.append(
                        ProtocolMessage('CachedRect', cache_id=cache_id, rect=rect)
                    )
                    parsed.cached_rect_count += 1

                elif 'requestcacheddata' in lower or 'requesting from server' in lower:
                    cache_id = parse_cache_id(message)
                    parsed.protocol_messages.append(
                        ProtocolMessage('RequestCachedData', cache_id=cache_id)
                    )
                    parsed.request_cached_data_count += 1
            
            # Eviction notifications (PersistentCache)
            if 'sending' in lower and 'persistentcache' in lower and 'eviction' in lower:
                # Parse: "Sending 5 PersistentCache eviction notifications"
                match = re.search(r'sending\s+(\d+).*persistentcache.*eviction', message, re.IGNORECASE)
                if match:
                    count = int(match.group(1))
                    parsed.persistent_eviction_count += 1
                    parsed.persistent_evicted_ids += count

            # Eviction notifications (ContentCache)
            elif 'sending' in lower and 'cache eviction' in lower and 'persistentcache' not in lower:
                # Parse: "Sending 5 cache eviction notifications to server"
                match = re.search(r'sending\s+(\d+)\s+cache\s+eviction', message, re.IGNORECASE)
                if match:
                    count = int(match.group(1))
                    parsed.cache_eviction_count += 1
                    parsed.evicted_ids_count += count
            
            # Server-side receipt (SMsgReader) line: "Client evicted N persistent cache entries"
            elif 'client evicted' in lower and 'persistent' in lower and 'cache' in lower:
                match = re.search(r'client\s+evicted\s+(\d+)', message, re.IGNORECASE)
                if match:
                    count = int(match.group(1))
                    parsed.persistent_eviction_count += 1
                    parsed.persistent_evicted_ids += count
            
            # PersistentCache bandwidth summary (viewer-side)
            elif 'persistentcache:' in lower:
                # Format: "PersistentCache: <size> bandwidth saving (98.4% reduction)"
                # Logs may wrap the word "reduction" onto the next line, so only
                # require that the percentage appears inside parentheses.
                m = re.search(r'PersistentCache:.*\(([-\d.]+)%', message, re.IGNORECASE)
                if m:
                    try:
                        parsed.persistent_bandwidth_reduction = float(m.group(1))
                    except ValueError:
                        pass
            
            # ContentCache bandwidth summary (viewer-side)
            elif 'contentcache:' in lower:
                # Format: "ContentCache: <size> bandwidth saving (98.4% reduction)"
                m = re.search(r'ContentCache:.*\(([-\d.]+)%', message, re.IGNORECASE)
                if m:
                    try:
                        parsed.content_bandwidth_reduction = float(m.group(1))
                    except ValueError:
                        pass
            
            # PersistentCache hit/miss counters from viewer logs
            # NOTE: Only HIT and MISS messages are counted for hit rate.
            # STORE messages (from PersistentCachedRectInit) are NOT counted
            # as misses because they represent initial population, not lookups.
            # The viewer only counts "lookups" when it receives a
            # PersistentCachedRect reference and checks its local cache.
            elif 'persistentcache' in lower and 'hit' in lower:
                parsed.persistent_hits += 1
            elif 'persistentcache' in lower and 'miss' in lower:
                parsed.persistent_misses += 1
            # PersistentCache STORE is initial population - track separately
            # but don't add to misses (it's not a cache lookup).
            elif 'persistentcache store' in lower:
                # Don't count as miss - STORE is initial population, not lookup
                pass
            
            elif 'evicted' in lower and 'cache' in lower:
                # Client or server eviction logging
                cache_id = parse_cache_id(message)
                parsed.cache_operations.append(
                    CacheOperation('eviction', cache_id=cache_id, details=message)
                )
            
            # ARC statistics (end of session)
            if 'client-side contentcache statistics' in lower or 'client-side persistentcache statistics' in lower:
                # Start of stats block, parse following lines
                pass
            
            # Parse stats like "Lookups: 1234, Hits: 567 (45.9%)"
            match = re.search(r'lookups:\s*(\d+).*hits:\s*(\d+).*\(([\d.]+)%\)', message, re.IGNORECASE)
            if match:
                lookups, hits, hit_rate = match.groups()
                parsed.total_lookups = int(lookups)
                parsed.total_hits = int(hits)
            
            # Only accept protocol-miss counters from the "Protocol operations"
            # line ("Misses: N, Queries sent: ...") and avoid ARC/internal
            # counters like "Cache misses: N".
            match = re.search(r'misses:\s*(\d+)\s*,\s*queries\s+sent', message, re.IGNORECASE)
            if match:
                parsed.total_misses = int(match.group(1))
            
            # ARC state
            match = re.search(r't1.*:\s*(\d+).*t2.*:\s*(\d+)', message, re.IGNORECASE)
            if match:
                if parsed.final_arc is None:
                    parsed.final_arc = ARCSnapshot()
                parsed.final_arc.t1_size = int(match.group(1))
                parsed.final_arc.t2_size = int(match.group(2))
            
            # Errors
            if 'error' in lower or 'fatal' in lower:
                parsed.errors.append(message)
    
    return parsed


def parse_server_log(log_path: Path, verbose: bool = False) -> ParsedLog:
    """Parse server log file for cache activity."""
    parsed = ParsedLog()
    
    if not log_path.exists():
        parsed.errors.append(f"Server log file not found: {log_path}")
        return parsed
    
    # Track sample messages for debugging
    hit_samples = []
    miss_samples = []
    
    # Track bandwidth savings. The PersistentCache protocol now uses the
    # same 64-bit ID wire format and overhead as ContentCache:
    #   - PersistentCachedRect reference: 20 bytes (12 header + 8 ID)
    #   - PersistentCachedRectInit: 24 bytes (12 header + 8 ID + 4 encoding)
    persistent_bytes_saved = 0
    persistent_bytes_sent_as_ref = 0  # 20 bytes per PersistentCachedRect
    persistent_bytes_sent_full = 0    # 24-byte header per PersistentCachedRectInit
    content_bytes_saved = 0
    content_bytes_sent_as_ref = 0     # 20 bytes per CachedRect
    
    last_was_pc_hit = False  # Track if previous line was a PC HIT (for continuation lines)
    
    with open(log_path, 'r', errors='replace') as f:
        for line in f:
            line = line.strip()
            lower = line.lower()
            
            # PersistentCache HIT with bandwidth savings
            # Format (multi-line):
            #   "EncodeManager: PersistentCache protocol HIT: rect [x,y-x,y]"
            #   "              hash=... saved 10896 bytes"
            if 'persistentcache' in lower and 'hit' in lower:
                parsed.persistent_hits += 1
                if len(hit_samples) < 5:
                    hit_samples.append(line)
                last_was_pc_hit = True
            elif last_was_pc_hit and 'saved' in lower and 'bytes' in lower:
                # Continuation line with "saved N bytes"
                match = re.search(r'saved\s+(\d+)\s+bytes', lower)
                if match:
                    persistent_bytes_saved += int(match.group(1))
                    # Unified cache protocol: 20 bytes per PersistentCachedRect reference
                    persistent_bytes_sent_as_ref += 20
                last_was_pc_hit = False
            elif 'persistentcache' in lower and 'miss' in lower:
                parsed.persistent_misses += 1
                if len(miss_samples) < 5:
                    miss_samples.append(line)
                last_was_pc_hit = False
            # PersistentCache INIT (full send)
            # Format: "PersistentCache INIT: rect [x,y-x,y] hash=... (now known for session)"
            elif 'persistentcache init' in lower:
                # Treat each PersistentCachedRectInit as a protocol-level miss:
                # the server had to send full data because the client didn't
                # yet know this ID. This mirrors how CachedRectInit is
                # interpreted for ContentCache.
                # Unified cache protocol: 24-byte header per INIT (12 header + 8 ID + 4 encoding)
                persistent_bytes_sent_full += 24
                parsed.persistent_init_count += 1
                parsed.persistent_misses += 1
                last_was_pc_hit = False
            else:
                # Not a PC message, reset state
                if last_was_pc_hit and 'hash=' not in lower:
                    # Not a continuation line either
                    last_was_pc_hit = False
            
            # ContentCache operations on server
            if 'contentcache.*hit' in lower or 'cache.*hit.*id' in lower:
                parsed.total_hits += 1
                if len(hit_samples) < 5:
                    hit_samples.append(line)
            elif 'contentcache.*miss' in lower:
                parsed.total_misses += 1
                if len(miss_samples) < 5:
                    miss_samples.append(line)
    
    # Calculate PersistentCache bandwidth reduction
    # Without cache: would have sent all as full encodings
    # With cache: sent some as references (47B) instead of full encodings
    # Reduction = (bytes_saved - ref_overhead) / (bytes_saved) * 100
    if persistent_bytes_saved > 0:
        net_savings = persistent_bytes_saved - persistent_bytes_sent_as_ref
        # Bandwidth reduction = net savings / total bytes that would have been sent
        total_bytes_without_cache = persistent_bytes_saved + persistent_bytes_sent_full
        if total_bytes_without_cache > 0:
            parsed.persistent_bandwidth_reduction = 100.0 * net_savings / total_bytes_without_cache
    
    # Print debug info if verbose
    if verbose:
        print(f"\n[DEBUG] Server log parsing results:")
        print(f"  PersistentCache: {parsed.persistent_hits} hits, {parsed.persistent_misses} misses")
        print(f"  ContentCache: {parsed.total_hits} hits, {parsed.total_misses} misses")
        print(f"  PersistentCache bandwidth: saved={persistent_bytes_saved}B, ref_overhead={persistent_bytes_sent_as_ref}B")
        print(f"  PersistentCache reduction: {parsed.persistent_bandwidth_reduction:.1f}%")
        
        if hit_samples:
            print(f"\n  Sample HIT messages:")
            for sample in hit_samples[:3]:
                print(f"    {sample[:120]}..." if len(sample) > 120 else f"    {sample}")
        
        if miss_samples:
            print(f"\n  Sample MISS messages:")
            for sample in miss_samples[:3]:
                print(f"    {sample[:120]}..." if len(sample) > 120 else f"    {sample}")
    
    return parsed


def parse_rust_log(log_path: Path) -> ParsedLog:
    """
    Parse Rust viewer log file.
    
    Note: Rust logs may have slightly different format, but we normalize
    to the same structure.
    """
    # For now, use same parser as C++ with some adaptations
    # TODO: Once Rust viewer logging format is finalized, adapt as needed
    parsed = parse_cpp_log(log_path)
    
    # Rust-specific adjustments if needed
    # (e.g., different log format, different message patterns)
    
    return parsed


def compute_hit_rate(parsed: ParsedLog) -> float:
    """Compute cache hit rate as percentage."""
    total = parsed.total_hits + parsed.total_misses
    if total == 0:
        return 0.0
    return 100.0 * parsed.total_hits / total


def compute_metrics(parsed: ParsedLog) -> Dict:
    """
    Compute aggregate metrics from parsed log.
    
    Returns dict suitable for comparison and reporting.
    
    IMPORTANT: This function prioritizes viewer-reported stats (from "Lookups: N,
    Hits: M" lines) over counting protocol messages. The viewer's self-reported
    stats are accurate because:
    - ContentCache: "Lookups" = CachedRect received, "Hits" = local cache found data
    - CachedRectInit is initial population, NOT a lookup (so not counted as miss)
    
    Only fall back to protocol message counting when viewer stats are absent.
    """
    # If viewer reported explicit stats, trust them over protocol message counts.
    # The viewer reports Lookups/Hits/Misses in its summary, and these are accurate.
    # We only fall back to counting CachedRectInit as "misses" when parsing server
    # logs that don't have viewer-side stats.
    #
    # NOTE: viewer-reported total_misses=0 is VALID (no RequestCachedData sent).
    # CachedRectInit is NOT a miss - it's initial population before any lookups.
    # Only override if total_lookups is 0 (no viewer stats at all).
    
    viewer_reported_stats = (parsed.total_lookups > 0)
    
    if not viewer_reported_stats:
        # No viewer stats - fall back to protocol message counting (server logs)
        # In this case, CachedRectInit represents content the client didn't have.
        if parsed.cached_rect_init_count > 0 and parsed.total_misses == 0:
            parsed.total_misses = parsed.cached_rect_init_count
        
        # Recalculate lookups from hits + misses
        if parsed.total_lookups == 0:
            parsed.total_lookups = parsed.total_hits + parsed.total_misses
    
    # Stores should match CachedRectInit count when not explicitly logged.
    if parsed.total_stores == 0 and parsed.cached_rect_init_count > 0:
        parsed.total_stores = parsed.cached_rect_init_count

    # PersistentCache: Same logic - trust viewer stats if available.
    # For PersistentCache, the viewer reports hits/misses directly in
    # "PersistentCache HIT/MISS" lines, which we count separately.
    # Only use persistent_init_count as fallback for server-only logs.
    pc_viewer_stats = (parsed.persistent_hits > 0 or parsed.persistent_misses > 0)
    if not pc_viewer_stats and parsed.persistent_init_count > 0:
        # Server log only - treat init messages as "misses"
        parsed.persistent_misses = parsed.persistent_init_count

    # Compute persistent hit rate after normalization.
    p_total = parsed.persistent_hits + parsed.persistent_misses
    p_hit_rate = (100.0 * parsed.persistent_hits / p_total) if p_total > 0 else 0.0

    metrics = {
        'cache_operations': {
            'total_hits': parsed.total_hits,
            'total_misses': parsed.total_misses,
            'total_stores': parsed.total_stores,
            'total_lookups': parsed.total_lookups,
            'hit_rate': compute_hit_rate(parsed),
            'bandwidth_reduction_pct': parsed.content_bandwidth_reduction,
        },
        'protocol_messages': {
            'CachedRect': parsed.cached_rect_count,
            'CachedRectInit': parsed.cached_rect_init_count,
            'RequestCachedData': parsed.request_cached_data_count,
            'CacheEviction': parsed.cache_eviction_count,
            'EvictedIDs': parsed.evicted_ids_count,
        },
'persistent': {
            'eviction_count': parsed.persistent_eviction_count,
            'evicted_ids': parsed.persistent_evicted_ids,
            'bandwidth_reduction_pct': parsed.persistent_bandwidth_reduction,
            'hits': parsed.persistent_hits,
            'misses': parsed.persistent_misses,
            'hit_rate': p_hit_rate,
            'init_events': parsed.persistent_init_events,
        },
        'arc_state': {},
        'errors': len(parsed.errors),
    }
    
    if parsed.final_arc:
        metrics['arc_state'] = {
            't1_size': parsed.final_arc.t1_size,
            't2_size': parsed.final_arc.t2_size,
            'b1_size': parsed.final_arc.b1_size,
            'b2_size': parsed.final_arc.b2_size,
            'total_entries': parsed.final_arc.total_entries,
            'memory_mb': parsed.final_arc.memory_mb,
        }
    
    return metrics


def format_metrics_summary(metrics: Dict) -> str:
    """Format metrics as human-readable summary."""
    lines = []
    
    lines.append("ContentCache Metrics:")
    cache = metrics['cache_operations']
    lines.append(f"  Hits: {cache['total_hits']}, Misses: {cache['total_misses']}, Hit Rate: {cache['hit_rate']:.1f}%")
    lines.append(f"  Stores: {cache['total_stores']}, Lookups: {cache['total_lookups']}")
    
    lines.append("\nProtocol Messages:")
    proto = metrics['protocol_messages']
    lines.append(f"  CachedRect: {proto['CachedRect']}")
    lines.append(f"  CachedRectInit: {proto['CachedRectInit']}")
    lines.append(f"  RequestCachedData: {proto['RequestCachedData']}")
    lines.append(f"  CacheEviction: {proto['CacheEviction']} ({proto['EvictedIDs']} IDs evicted)")
    
    if metrics['arc_state']:
        lines.append("\nARC State:")
        arc = metrics['arc_state']
        lines.append(f"  T1: {arc.get('t1_size', 0)}, T2: {arc.get('t2_size', 0)}")
        lines.append(f"  Memory: {arc.get('memory_mb', 0):.1f} MB")
    
    if metrics['errors'] > 0:
        lines.append(f"\n⚠ Errors logged: {metrics['errors']}")
    
    return '\n'.join(lines)


if __name__ == '__main__':
    # Test/demo mode
    import sys
    if len(sys.argv) < 2:
        print("Usage: log_parser.py <log_file>")
        sys.exit(1)
    
    log_path = Path(sys.argv[1])
    parsed = parse_cpp_log(log_path)
    metrics = compute_metrics(parsed)
    
    print(format_metrics_summary(metrics))
    
    if parsed.errors:
        print("\nErrors found:")
        for err in parsed.errors[:10]:  # First 10 errors
            print(f"  - {err}")
