#!/usr/bin/env python3
"""Privileged WAN shaping helper for tc/netem.

This script is intended to be run with CAP_NET_ADMIN (for example, via a
systemd unit or other trusted mechanism). It exposes a very small CLI
that matches the expectations of `wanem.py` when
$TIGERVNC_WAN_HELPER is set:

  wanem_helper.py apply <profile> <dev> <port1> [<port2> ...]
  wanem_helper.py clear <dev>

The heavy lifting is delegated to the shared logic in `wanem.py` so that
all shaping rules remain defined in a single place.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

# Make sure we can import the sibling wanem module.
HERE = Path(__file__).resolve().parent
if str(HERE) not in sys.path:
    sys.path.insert(0, str(HERE))

from wanem import WAN_PROFILES, _configure_netem_and_filters, clear_wan_shaping  # type: ignore[attr-defined]


def cmd_apply(args: argparse.Namespace) -> int:
    profile = WAN_PROFILES.get(args.profile)
    if profile is None:
        print(f"ERROR: unknown WAN profile '{args.profile}'", file=sys.stderr)
        print(f"Available profiles: {', '.join(sorted(WAN_PROFILES))}", file=sys.stderr)
        return 1

    ports = []
    for p in args.ports:
        try:
            ports.append(int(p))
        except ValueError:
            print(f"ERROR: invalid port '{p}' (expected integer)", file=sys.stderr)
            return 1

    if not ports:
        print("ERROR: at least one port must be specified for 'apply'", file=sys.stderr)
        return 1

    ok = _configure_netem_and_filters(args.dev, profile, ports, verbose=True)
    return 0 if ok else 1


def cmd_clear(args: argparse.Namespace) -> int:
    clear_wan_shaping(dev=args.dev, verbose=True)
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Privileged WAN tc/netem helper")
    subparsers = parser.add_subparsers(dest="cmd", required=True)

    p_apply = subparsers.add_parser("apply", help="Apply WAN profile to ports on an interface")
    p_apply.add_argument("profile", help="Profile name defined in wanem.WAN_PROFILES")
    p_apply.add_argument("dev", help="Network device to shape (e.g. lo)")
    p_apply.add_argument("ports", nargs="+", help="TCP ports to shape")
    p_apply.set_defaults(func=cmd_apply)

    p_clear = subparsers.add_parser("clear", help="Clear WAN shaping from an interface")
    p_clear.add_argument("dev", help="Network device to clear (e.g. lo)")
    p_clear.set_defaults(func=cmd_clear)

    args = parser.parse_args(argv)
    return args.func(args)


if __name__ == "__main__":  # pragma: no cover
    raise SystemExit(main())
