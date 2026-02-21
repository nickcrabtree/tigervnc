# Prompt for a new agent (copy/paste)

You are taking over a TigerVNC PersistentCache debugging/build session in Nick’s environment.

**Attachments provided:**

1) `TigerVNC_PersistentCache_Playbook.md`

2) `TigerVNC_PersistentCache_Handover.md`

**Your tasks:**

- Read the playbook first to understand environment constraints, safe workflows, and known pitfalls.

- Read the handover to understand what has already been fixed and what remains.

**Constraints & preferences:**

- Prefer generating **patch artifacts** against uploaded current files rather than giving source-edit scripts.

- Provide patches as downloadable `*.patch.txt` plus `*.sha256.txt` and (if possible) the structural checker output.

- Nick runs the viewer via wrapper `~/scripts/njcvncviewer_start.sh`.

**Immediate next step:**

- Run the **fresh-cache missing-shard self-heal test** using `-PersistentCachePath` to point to a new directory under `/Volumes/Nick/tmp/pcache_fresh_<ts>`.

- Keep disk cap at default (~4GB): do **not** set `-PersistentCacheDiskSize`.

- Collect `/tmp/njcvncviewer_*.log` and `/tmp/persistentcache_debug_*.log` and summarize outcomes.

**When proposing commands:**

- Provide a single copy/paste block.

- Wrap it in a subshell and tee output to a log file under `/Volumes/Nick/tmp/`.

- Avoid dangerous scripted source rewrites.

Proceed with the test plan and report results + next recommendations.
