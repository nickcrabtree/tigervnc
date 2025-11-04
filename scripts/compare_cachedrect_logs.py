#!/usr/bin/env python3
import argparse
import re
import sys
from pathlib import Path


def read(p):
    return Path(p).read_text(errors="ignore")


def grep_count(text, pat):
    return len(re.findall(pat, text, flags=re.IGNORECASE))


def main():
    ap = argparse.ArgumentParser(
        description="Compare server and viewer logs for ContentCache/CachedRectInit flow"
    )
    ap.add_argument("--server", required=True, help="Server log (contentcache_test.log)")
    ap.add_argument("--client", required=True, help="Viewer log")
    args = ap.parse_args()

    s = read(args.server)
    c = read(args.client)

    # Server metrics
    hits = grep_count(s, r"ContentCache protocol hit: rect")
    refs_match = re.findall(r"Lookups:\s*(\d+),\s*References sent:\s*(\d+)", s)
    lookups = refs_match[-1][0] if refs_match else "0"
    refs = int(refs_match[-1][1]) if refs_match else 0
    srv_requests = grep_count(s, r"Client requested cached data for ID\s+\d+")
    
    # Client metrics
    cli_misses = grep_count(c, r"Cache miss for ID\s+\d+")
    cli_stores = grep_count(c, r"Storing decoded rect .* cache ID\s+\d+")
    cli_cachedrect = grep_count(c, r"Received CachedRect[: ]")
    cli_init = grep_count(c, r"CachedRectInit")  # best-effort
    
    # Negotiation checks
    advertised_cache = bool(
        re.search(r"SetEncodings.*(CachedRect|ContentCache|EncCache)", c, re.IGNORECASE)
    )
    unknown_enc = grep_count(c, r"unknown encoding|unknown message|unsupported encoding")

    print("=== Summary ===")
    print(
        f"Server: Lookups={lookups}, References sent={refs}, Hits(log lines)={hits}, "
        f"Requests from client={srv_requests}"
    )
    print(
        f"Client: CachedRect refs={cli_cachedrect}, Cache misses={cli_misses}, "
        f"Stores (decoded+cached)={cli_stores}, CachedRectInit mentions={cli_init}"
    )
    print(f"Client advertised ContentCache in SetEncodings: {advertised_cache}")
    if unknown_enc:
        print(
            f"Client reported unknown/unsupported encodings/messages: {unknown_enc} occurrence(s)"
        )

    print("\n=== Diagnostics ===")
    problems = 0
    
    if refs > 0 and cli_stores == 0 and cli_misses == 0:
        print(
            "- Viewer received no CachedRectInit and did not log cache misses. "
            "Suspect: viewer did not advertise ContentCache capability."
        )
        if not advertised_cache:
            print("  -> CONFIRMED: Viewer did not advertise ContentCache in SetEncodings.")
        problems += 1
        
    if cli_misses > 0 and cli_stores == 0:
        print("- Viewer requested cached data on misses but did not store any decoded rects.")
        print(
            "  -> Suspect: server did not respond with CachedRectInit or "
            "response dropped/misparsed."
        )
        problems += 1
        
    if refs == 0:
        print(
            "- Server sent 0 References; scenario might not have triggered cache hits "
            "or ContentCache disabled on server."
        )
        problems += 1
        
    if unknown_enc:
        print(
            "- Protocol mismatch: viewer saw unknown/unsupported encoding(s). "
            "Check negotiated encodings and build options."
        )
        problems += 1
        
    if problems == 0:
        print(
            "- No obvious issues detected. Inspect timelines for interleaving and "
            "verify that the viewer connected before/during the scenario."
        )

    print("\n=== Next steps if viewer didn't advertise ContentCache ===")
    print(
        "* Rebuild viewer ensuring ContentCache is compiled in (it should be by default). "
        "Verify with verbose logs and ensure 'SetEncodings' contains CachedRect/ContentCache."
    )
    print(
        "* Check CMake options and that common/rfb/ContentCache*.cxx compiled; "
        "look for 'ContentCache' strings in the binary: strings njcvncviewer | grep -i contentcache"
    )


if __name__ == "__main__":
    sys.exit(main())
