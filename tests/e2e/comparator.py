#!/usr/bin/env python3
"""
Comparator for ContentCache metrics between C++ and Rust viewers.

Implements tolerance-based comparison with configurable thresholds.
"""

from typing import Dict, List, Tuple
from dataclasses import dataclass, field


@dataclass
class ComparisonResult:
    """Result of metrics comparison."""
    passed: bool
    failures: List[str] = field(default_factory=list)
    warnings: List[str] = field(default_factory=list)
    details: Dict = field(default_factory=dict)


@dataclass
class Tolerances:
    """Configurable tolerance thresholds."""
    hit_rate_pct: float = 2.0  # ±2% absolute
    protocol_message_pct: float = 5.0  # ±5%
    arc_balance_pct: float = 10.0  # ±10%


def compare_hit_rates(baseline: Dict, candidate: Dict, tolerance: float = 2.0) -> Tuple[bool, str]:
    """
    Compare cache hit rates.
    
    Args:
        baseline: Baseline metrics dict
        candidate: Candidate metrics dict
        tolerance: Absolute percentage point tolerance
    
    Returns:
        (passed, message)
    """
    base_rate = baseline['cache_operations']['hit_rate']
    cand_rate = candidate['cache_operations']['hit_rate']
    
    diff = abs(base_rate - cand_rate)
    
    if diff <= tolerance:
        return True, f"Hit rates match: {base_rate:.1f}% vs {cand_rate:.1f}% (Δ={diff:.1f}%)"
    else:
        return False, f"Hit rates differ: {base_rate:.1f}% vs {cand_rate:.1f}% (Δ={diff:.1f}%, tolerance={tolerance}%)"


def compare_protocol_messages(baseline: Dict, candidate: Dict, tolerance_pct: float = 5.0) -> Tuple[bool, List[str]]:
    """
    Compare protocol message counts.
    
    Returns:
        (passed, messages)
    """
    passed = True
    messages = []
    
    base_proto = baseline['protocol_messages']
    cand_proto = candidate['protocol_messages']
    
    for msg_type in ['CachedRect', 'CachedRectInit', 'RequestCachedData']:
        base_count = base_proto.get(msg_type, 0)
        cand_count = cand_proto.get(msg_type, 0)
        
        if base_count == 0 and cand_count == 0:
            messages.append(f"  {msg_type}: both 0 (OK)")
            continue
        
        if base_count == 0:
            messages.append(f"  {msg_type}: baseline=0 but candidate={cand_count} (WARNING)")
            continue
        
        diff_pct = abs(cand_count - base_count) / base_count * 100
        
        if diff_pct <= tolerance_pct:
            messages.append(f"  {msg_type}: {base_count} vs {cand_count} (Δ={diff_pct:.1f}%, OK)")
        else:
            passed = False
            messages.append(f"  {msg_type}: {base_count} vs {cand_count} (Δ={diff_pct:.1f}%, FAIL tolerance={tolerance_pct}%)")
    
    return passed, messages


def compare_arc_balance(baseline: Dict, candidate: Dict, tolerance_pct: float = 10.0) -> Tuple[bool, List[str]]:
    """
    Compare ARC T1/T2 balance.
    
    Returns:
        (passed, messages)
    """
    base_arc = baseline.get('arc_state', {})
    cand_arc = candidate.get('arc_state', {})
    
    if not base_arc or not cand_arc:
        return True, ["ARC state not available for comparison (skipped)"]
    
    messages = []
    passed = True
    
    base_t1 = base_arc.get('t1_size', 0)
    base_t2 = base_arc.get('t2_size', 0)
    cand_t1 = cand_arc.get('t1_size', 0)
    cand_t2 = cand_arc.get('t2_size', 0)
    
    base_total = base_t1 + base_t2
    cand_total = cand_t1 + cand_t2
    
    if base_total > 0:
        base_t1_pct = 100.0 * base_t1 / base_total
    else:
        base_t1_pct = 0.0
    
    if cand_total > 0:
        cand_t1_pct = 100.0 * cand_t1 / cand_total
    else:
        cand_t1_pct = 0.0
    
    balance_diff = abs(base_t1_pct - cand_t1_pct)
    
    if balance_diff <= tolerance_pct:
        messages.append(f"  ARC T1/T2 balance: {base_t1_pct:.1f}%/{100-base_t1_pct:.1f}% vs {cand_t1_pct:.1f}%/{100-cand_t1_pct:.1f}% (OK)")
    else:
        passed = False
        messages.append(f"  ARC balance differs: {base_t1_pct:.1f}%/{100-base_t1_pct:.1f}% vs {cand_t1_pct:.1f}%/{100-cand_t1_pct:.1f}% (FAIL)")
    
    return passed, messages


def compare_metrics(baseline: Dict, candidate: Dict, tolerances: Tolerances = None) -> ComparisonResult:
    """
    Compare baseline (C++) vs candidate (Rust) metrics.
    
    Args:
        baseline: Parsed metrics from C++ viewer
        candidate: Parsed metrics from Rust viewer
        tolerances: Tolerance thresholds
    
    Returns:
        ComparisonResult with pass/fail and detailed messages
    """
    if tolerances is None:
        tolerances = Tolerances()
    
    result = ComparisonResult(passed=True)
    
    # Compare hit rates
    hit_passed, hit_msg = compare_hit_rates(baseline, candidate, tolerances.hit_rate_pct)
    if hit_passed:
        result.details['hit_rate'] = 'PASS'
    else:
        result.passed = False
        result.failures.append(hit_msg)
        result.details['hit_rate'] = 'FAIL'
    
    # Compare protocol messages
    proto_passed, proto_msgs = compare_protocol_messages(baseline, candidate, tolerances.protocol_message_pct)
    if proto_passed:
        result.details['protocol_messages'] = 'PASS'
    else:
        result.passed = False
        result.failures.extend(proto_msgs)
        result.details['protocol_messages'] = 'FAIL'
    
    # Compare ARC balance
    arc_passed, arc_msgs = compare_arc_balance(baseline, candidate, tolerances.arc_balance_pct)
    if arc_passed:
        result.details['arc_balance'] = 'PASS'
    else:
        result.passed = False
        result.failures.extend(arc_msgs)
        result.details['arc_balance'] = 'FAIL'
    
    # Check for errors
    base_errors = baseline.get('errors', 0)
    cand_errors = candidate.get('errors', 0)
    
    if base_errors > 0:
        result.warnings.append(f"Baseline had {base_errors} errors")
    if cand_errors > 0:
        result.warnings.append(f"Candidate had {cand_errors} errors")
    
    return result


def format_comparison_result(result: ComparisonResult) -> str:
    """Format comparison result as human-readable text."""
    lines = []
    
    if result.passed:
        lines.append("✓ PASS: Candidate matches baseline within tolerances")
    else:
        lines.append("✗ FAIL: Candidate does not match baseline within tolerances")
    
    if result.failures:
        lines.append("\nFailures:")
        for failure in result.failures:
            lines.append(f"  - {failure}")
    
    if result.warnings:
        lines.append("\nWarnings:")
        for warning in result.warnings:
            lines.append(f"  ⚠ {warning}")
    
    return '\n'.join(lines)
