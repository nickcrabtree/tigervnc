#!/usr/bin/env python3
"""Simple WAN emulation helper for e2e tests using Linux tc/netem.

This module does *not* try to be a full network emulator. It provides a thin,
opt‑in wrapper over `tc netem` that can be used by the e2e harness to inject
latency, jitter, loss and bandwidth limits on specific localhost TCP ports.

Design goals
------------

- Keep all shaping confined to a single interface (default: `lo`).
- Only match the explicit TCP ports used by a given test run.
- Be tolerant of missing `tc` or insufficient privileges (fail with a clear
  message instead of crashing).
- Provide named "profiles" that can be selected from the test CLI.

Safety notes
------------

- This helper modifies qdisc configuration on the chosen interface. It should
  only be used on development machines.
- The implementation is conservative: it either attaches its own root qdisc
  (when none exists) or bails out if an unknown existing qdisc is present.
- All commands use short timeouts to avoid hanging test runs.

Typical usage from a test harness
---------------------------------

    from wanem import WAN_PROFILES, apply_wan_profile, clear_wan_shaping

    profile = args.wan_profile  # e.g. "3g" or "bad_wifi"
    shaped_ports = [args.port_content, args.port_viewer]
    shaping_active = False
    if profile:
        shaping_active = apply_wan_profile(profile, shaped_ports,
                                           dev="lo", verbose=args.verbose)

    try:
        ... run tests ...
    finally:
        if shaping_active:
            clear_wan_shaping("lo", verbose=args.verbose)
"""

from __future__ import annotations

import shutil
import subprocess
from dataclasses import dataclass
from typing import Dict, Iterable, List, Optional


TC_BIN = shutil.which("tc") or "tc"


@dataclass(frozen=True)
class WanProfile:
    """High‑level description of a WAN profile.

    All values are approximate and mapped directly to `tc netem`/`tbf`.
    """

    # One‑way base latency in milliseconds
    delay_ms: int = 0
    # Plus/minus jitter in milliseconds (0 = no jitter)
    jitter_ms: int = 0
    # Random packet loss percentage (0.0‑100.0)
    loss_pct: float = 0.0
    # Random packet duplication percentage
    duplicate_pct: float = 0.0
    # Random packet reordering percentage
    reorder_pct: float = 0.0
    # Approximate bandwidth cap in kbit/s (None = unlimited)
    rate_kbit: Optional[int] = None


# A small set of reasonable defaults for interactive remote desktops.
WAN_PROFILES: Dict[str, WanProfile] = {
    "none": WanProfile(),
    # Light home broadband / office Wi‑Fi
    "wifi_good": WanProfile(delay_ms=20, jitter_ms=5, loss_pct=0.1, rate_kbit=20000),
    # Congested or distant Wi‑Fi
    "wifi_bad": WanProfile(delay_ms=60, jitter_ms=30, loss_pct=1.0,
                             reorder_pct=0.5, rate_kbit=5000),
    # 4G / basic LTE like
    "4g": WanProfile(delay_ms=50, jitter_ms=20, loss_pct=0.5, rate_kbit=10000),
    # Older 3G link
    "3g": WanProfile(delay_ms=120, jitter_ms=40, loss_pct=1.5, rate_kbit=3000),
    # High‑latency satellite / remote link with modest loss
    "satellite": WanProfile(delay_ms=300, jitter_ms=60, loss_pct=0.5,
                             reorder_pct=1.0, rate_kbit=1500),
}


def _run_tc(args: List[str], timeout: float = 5.0, verbose: bool = False) -> bool:
    """Run a `tc` command, returning True on success.

    This helper centralises timeout handling and error reporting so that
    callers can fail gracefully without raising exceptions.
    """

    cmd = [TC_BIN] + args
    if verbose:
        print(f"[wanem] tc {' '.join(args)}")

    try:
        subprocess.run(
            cmd,
            check=True,
            timeout=timeout,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        return True
    except subprocess.TimeoutExpired:
        print(f"[wanem] ERROR: 'tc {' '.join(args)}' timed out after {timeout}s")
    except (OSError, subprocess.CalledProcessError) as exc:
        print(f"[wanem] ERROR: 'tc {' '.join(args)}' failed: {exc}")
    return False


def _ensure_root_qdisc(dev: str, verbose: bool = False) -> bool:
    """Ensure there is a root qdisc we control on `dev`.

    Strategy:
      * If no qdisc is present, attach `prio` as handle 1:.
      * If a `prio` qdisc with handle 1: already exists, reuse it.
      * Otherwise, leave the existing qdisc alone and refuse to proceed.
    """

    # Query existing qdisc configuration.
    try:
        result = subprocess.run(
            [TC_BIN, "qdisc", "show", "dev", dev],
            capture_output=True,
            text=True,
            timeout=5.0,
        )
    except Exception as exc:  # pragma: no cover - defensive
        print(f"[wanem] ERROR: Failed to inspect qdisc on {dev}: {exc}")
        return False

    if result.returncode != 0:
        # Assume no qdisc; attempt to add our own.
        return _run_tc(["qdisc", "add", "dev", dev, "root", "handle", "1:", "prio"],
                       verbose=verbose)

    lines = result.stdout.splitlines()
    # Look for a root qdisc line.
    for line in lines:
        if "qdisc" not in line:
            continue
        if "root" not in line:
            continue
        if "prio" in line and "1:" in line:
            # Compatible existing root qdisc; reuse.
            if verbose:
                print(f"[wanem] Reusing existing root prio qdisc on {dev} (handle 1:)")
            return True
        # If we find some other root qdisc that isn't ours, bail out.
        if "1:" in line and "prio" not in line:
            print(
                f"[wanem] ERROR: Unsupported existing root qdisc on {dev}:\n"
                f"  {line}\n"
                "[wanem] Refusing to modify qdisc; please run tests on a dedicated "
                "development machine or remove custom qdisc config first."
            )
            return False

    # No recognised root; attach our own.
    return _run_tc(["qdisc", "add", "dev", dev, "root", "handle", "1:", "prio"],
                   verbose=verbose)


def _configure_netem_and_filters(
    dev: str,
    profile: WanProfile,
    ports: Iterable[int],
    verbose: bool = False,
) -> bool:
    """Attach netem/tbf to handle 1:3 and filter selected TCP ports into it.

    Layout:
      * root qdisc: prio 1:
      * shaped class: 1:3 with child netem (+ optional tbf)
      * filters: u32 match ip sport/dport == port -> flowid 1:3
    """

    if not _ensure_root_qdisc(dev, verbose=verbose):
        return False

    # First remove any previous 1:3 qdisc so we can reconfigure cleanly.
    _run_tc(["qdisc", "del", "dev", dev, "parent", "1:3", "handle", "30:", "netem"],
            verbose=verbose)
    _run_tc(["qdisc", "del", "dev", dev, "parent", "30:", "handle", "40:", "tbf"],
            verbose=verbose)

    # Build netem arguments.
    netem_args: List[str] = [
        "qdisc",
        "add",
        "dev",
        dev,
        "parent",
        "1:3",
        "handle",
        "30:",
        "netem",
    ]

    if profile.delay_ms > 0:
        netem_args.extend(["delay", f"{profile.delay_ms}ms"])
        if profile.jitter_ms > 0:
            netem_args.extend([f"{profile.jitter_ms}ms", "distribution", "normal"])
    if profile.loss_pct > 0.0:
        netem_args.extend(["loss", f"{profile.loss_pct}%"])
    if profile.duplicate_pct > 0.0:
        netem_args.extend(["duplicate", f"{profile.duplicate_pct}%"])
    if profile.reorder_pct > 0.0:
        netem_args.extend(["reorder", f"{profile.reorder_pct}%"])

    if not _run_tc(netem_args, verbose=verbose):
        return False

    # Optional rate limiting via tbf stacked after netem.
    if profile.rate_kbit is not None and profile.rate_kbit > 0:
        # A small burst/latency bucket; values here are conservative.
        rate = f"{profile.rate_kbit}kbit"
        burst = "32kbit"
        latency = "400ms"
        tbf_args = [
            "qdisc",
            "add",
            "dev",
            dev,
            "parent",
            "30:",
            "handle",
            "40:",
            "tbf",
            "rate",
            rate,
            "burst",
            burst,
            "latency",
            latency,
        ]
        if not _run_tc(tbf_args, verbose=verbose):
            return False

    # Clear existing filters that may target 1:3; best‑effort only.
    _run_tc(["filter", "del", "dev", dev, "parent", "1:", "prio", "3"],
            verbose=verbose)

    ok = True
    for port in ports:
        if port <= 0:
            continue
        # Egress: packets leaving this host with dport == port.
        if not _run_tc(
            [
                "filter",
                "add",
                "dev",
                dev,
                "protocol",
                "ip",
                "parent",
                "1:",
                "prio",
                "3",
                "u32",
                "match",
                "ip",
                "dport",
                str(port),
                "0xffff",
                "flowid",
                "1:3",
            ],
            verbose=verbose,
        ):
            ok = False
        # Ingress (responses): sport == port.
        if not _run_tc(
            [
                "filter",
                "add",
                "dev",
                dev,
                "protocol",
                "ip",
                "parent",
                "1:",
                "prio",
                "3",
                "u32",
                "match",
                "ip",
                "sport",
                str(port),
                "0xffff",
                "flowid",
                "1:3",
            ],
            verbose=verbose,
        ):
            ok = False

    return ok


def apply_wan_profile(
    profile_name: str,
    ports: Iterable[int],
    dev: str = "lo",
    verbose: bool = False,
) -> bool:
    """Apply the named WAN profile to the given TCP ports on `dev`.

    Returns True if shaping was successfully configured, False otherwise.
    If `profile_name` is "none", any existing shaping managed by this helper
    on `dev` is removed and False is returned (to signal that nothing is
    currently active).
    """

    if profile_name not in WAN_PROFILES:
        print(f"[wanem] ERROR: Unknown WAN profile '{profile_name}'.")
        print(f"[wanem] Available profiles: {', '.join(sorted(WAN_PROFILES))}")
        return False

    if profile_name == "none":
        clear_wan_shaping(dev, verbose=verbose)
        return False

    if shutil.which(TC_BIN) is None and TC_BIN == "tc":
        print("[wanem] ERROR: 'tc' binary not found; install iproute2 to use WAN emulation.")
        return False

    profile = WAN_PROFILES[profile_name]
    print(
        f"[wanem] Applying WAN profile '{profile_name}' on {dev} "
        f"for ports {sorted(set(ports))}: delay={profile.delay_ms}ms "
        f"jitter={profile.jitter_ms}ms loss={profile.loss_pct}% "
        f"rate={profile.rate_kbit or 'unlimited'}kbit"
    )

    return _configure_netem_and_filters(dev, profile, ports, verbose=verbose)


def clear_wan_shaping(dev: str = "lo", verbose: bool = False) -> None:
    """Best‑effort removal of shaping created by this helper on `dev`.

    This removes the child qdiscs (netem/tbf) and the prio root qdisc with
    handle 1: **only** if it appears to be one we created. If a different
    qdisc configuration is present, this function leaves it untouched.
    """

    # Remove child qdiscs first.
    _run_tc(["qdisc", "del", "dev", dev, "parent", "1:3", "handle", "30:", "netem"],
            verbose=verbose)
    _run_tc(["qdisc", "del", "dev", dev, "parent", "30:", "handle", "40:", "tbf"],
            verbose=verbose)

    # Only delete the root if it is the simple prio 1: we expect.
    try:
        result = subprocess.run(
            [TC_BIN, "qdisc", "show", "dev", dev],
            capture_output=True,
            text=True,
            timeout=5.0,
        )
    except Exception:  # pragma: no cover - defensive
        return

    if result.returncode != 0:
        return

    for line in result.stdout.splitlines():
        if "root" in line and "prio" in line and "1:" in line:
            _run_tc(["qdisc", "del", "dev", dev, "root", "handle", "1:"],
                    verbose=verbose)
            break
