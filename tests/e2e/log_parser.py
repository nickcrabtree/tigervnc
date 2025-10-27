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


@dataclass
class ParsedLog:
    """Parsed and normalized log data."""
    cache_operations: List[CacheOperation] = field(default_factory=list)
    protocol_messages: List[ProtocolMessage] = field(default_factory=list)
    arc_snapshots: List[ARCSnapshot] = field(default_factory=list)
    errors: List[str] = field(default_factory=list)
    
    # Aggregate counts
    total_hits: int = 0
    total_misses: int = 0
    total_stores: int = 0
    total_lookups: int = 0
    
    # Message counts
    cached_rect_count: int = 0
    cached_rect_init_count: int = 0
    request_cached_data_count: int = 0
    
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
            
            # ContentCache operations
            if 'cache hit' in message.lower():
                cache_id = parse_cache_id(message)
                rect = parse_rect_coords(message)
                parsed.cache_operations.append(
                    CacheOperation('hit', cache_id=cache_id, rect=rect, details=message)
                )
                parsed.total_hits += 1
            
            elif 'cache miss' in message.lower():
                cache_id = parse_cache_id(message)
                parsed.cache_operations.append(
                    CacheOperation('miss', cache_id=cache_id, details=message)
                )
                parsed.total_misses += 1
            
            elif 'storing decoded rect' in message.lower() or 'store' in message.lower() and 'cache' in message.lower():
                cache_id = parse_cache_id(message)
                rect = parse_rect_coords(message)
                parsed.cache_operations.append(
                    CacheOperation('store', cache_id=cache_id, rect=rect, details=message)
                )
                parsed.total_stores += 1
            
            # Protocol messages
            if 'cachedrectinit' in message.lower():
                cache_id = parse_cache_id(message)
                rect = parse_rect_coords(message)
                parsed.protocol_messages.append(
                    ProtocolMessage('CachedRectInit', cache_id=cache_id, rect=rect)
                )
                parsed.cached_rect_init_count += 1
            
            elif 'cachedrect' in message.lower() and 'init' not in message.lower():
                cache_id = parse_cache_id(message)
                rect = parse_rect_coords(message)
                parsed.protocol_messages.append(
                    ProtocolMessage('CachedRect', cache_id=cache_id, rect=rect)
                )
                parsed.cached_rect_count += 1
            
            elif 'requestcacheddata' in message.lower() or 'requesting from server' in message.lower():
                cache_id = parse_cache_id(message)
                parsed.protocol_messages.append(
                    ProtocolMessage('RequestCachedData', cache_id=cache_id)
                )
                parsed.request_cached_data_count += 1
            
            # ARC statistics (end of session)
            if 'client-side contentcache statistics' in message.lower():
                # Start of stats block, parse following lines
                pass
            
            # Parse stats like "Lookups: 1234, Hits: 567 (45.9%)"
            match = re.search(r'lookups:\s*(\d+).*hits:\s*(\d+).*\(([\d.]+)%\)', message, re.IGNORECASE)
            if match:
                lookups, hits, hit_rate = match.groups()
                parsed.total_lookups = int(lookups)
                parsed.total_hits = int(hits)
            
            match = re.search(r'misses:\s*(\d+)', message, re.IGNORECASE)
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
            if 'error' in message.lower() or 'fatal' in message.lower():
                parsed.errors.append(message)
    
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
    """
    metrics = {
        'cache_operations': {
            'total_hits': parsed.total_hits,
            'total_misses': parsed.total_misses,
            'total_stores': parsed.total_stores,
            'total_lookups': parsed.total_lookups,
            'hit_rate': compute_hit_rate(parsed),
        },
        'protocol_messages': {
            'CachedRect': parsed.cached_rect_count,
            'CachedRectInit': parsed.cached_rect_init_count,
            'RequestCachedData': parsed.request_cached_data_count,
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
