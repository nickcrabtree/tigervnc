#!/usr/bin/env python3
"""TigerVNC e2e viewer wrapper: normalise argv order for njcvncviewer.

Many e2e tests historically invoke the viewer as:
    njcvncviewer <host> Param=Value Param=Value ...
but njcvncviewer expects parameters before the host:
    njcvncviewer Param=Value ... <host>

This wrapper detects the common host-first pattern and rewrites argv to
parameters-first, then execs the real viewer binary.

The real viewer path is supplied via $TIGERVNC_REAL_CPP_VIEWER, set by
tests/e2e/framework.py when it selects the default C++ viewer build output.
"""

import os
import sys


def _looks_like_param(tok: str) -> bool:
    # e2e harness commonly uses bare 'Key=Value' tokens (no leading '-') as well
    # as '-Key=Value' forms. '=' is a strong signal of a parameter.
    if "=" in tok:
        return True
    # Allow conventional options as well (rare in our tests).
    if tok.startswith("-") and len(tok) > 1:
        return True
    return False


def _normalise(argv: list[str]) -> list[str]:
    if not argv:
        return argv
    first = argv[0]
    # If the first token does not look like a parameter, and any later token does,
    # treat the first token as the host and move it to the end.
    if (not _looks_like_param(first)) and any(_looks_like_param(a) for a in argv[1:]):
        return argv[1:] + [first]
    return argv


def main() -> int:
    real = os.environ.get("TIGERVNC_REAL_CPP_VIEWER")
    if not real:
        print("ERROR: TIGERVNC_REAL_CPP_VIEWER is not set; cannot locate real viewer.", file=sys.stderr)
        return 2

    argv = sys.argv[1:]
    new_argv = _normalise(argv)

    if os.environ.get("TIGERVNC_VIEWER_WRAPPER_DEBUG", "").strip() not in ("", "0"):
        print(f"[njcvncviewer_wrapper] real={real}", file=sys.stderr)
        print(f"[njcvncviewer_wrapper] in ={argv}", file=sys.stderr)
        print(f"[njcvncviewer_wrapper] out={new_argv}", file=sys.stderr)

    os.execv(real, [real] + new_argv)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
