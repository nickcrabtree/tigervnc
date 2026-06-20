# Desloppify: Code Quality — tigervnc (rust-vnc-viewer)

## Conda environment

```bash
conda run -n desloppify desloppify <command>
```

Always run from the repo root (`/home/nickc/code/tigervnc`).
Desloppify is scoped to `rust-vnc-viewer/` only — the upstream C++ codebase is excluded.
Never use bare `desloppify` — always prefix with `conda run -n desloppify`.

---

## Scores as of 2026-05-25 (first scan)

| Metric | Score | Notes |
|---|---|---|
| Overall (lenient) | 22.8 / 100 | Low because subjective dimensions unreviewed |
| Objective (mechanical) | **91.4 / 100** | Above 80 target — mechanical is good |
| Strict (penalises wontfix) | 22.8 / 100 | Will rise sharply after subjective review |
| Verified (scan-confirmed) | 91.4 / 100 | |

Target: strict 85.0. Scoped to `rust-vnc-viewer/`. Last scan: 2026-05-25.

**The strict score is almost entirely driven by subjective dimensions being unreviewed (all
score 0%). Mechanical quality is good. Running the subjective review is the highest-leverage action.**

---

## Open mechanical issues (140 in scope)

| Dimension | Health | Notes |
|---|---|---|
| Code quality | 96.2% | Minor — autofix available |
| Security | 100.0% | Clean |
| File health | 81.4% | Focus here — 21 items to fix |
| Duplication | 99.3% | Clean |
| Test health | 83.2% | Good |

Run `desloppify next --cluster` to get the prioritised fix list.

---

## How the scan is scoped

The first scan was run with `--path rust-vnc-viewer`:

```bash
cd /home/nickc/code/tigervnc
conda run -n desloppify desloppify scan --path rust-vnc-viewer
```

All subsequent scans and status checks should also be run from the repo root
(desloppify auto-detects the project from `.desloppify/` state).

---

## Next steps

1. **Run the subjective review** — highest-leverage action.
   See the workflow below.

2. **Fix file health issues** (21 items, 81.4%):

   ```bash
   conda run -n desloppify desloppify next
   ```

3. **Rescan** after fixes:

   ```bash
   conda run -n desloppify desloppify scan
   ```

---

## How to run desloppify

### Check current score

```bash
conda run -n desloppify desloppify status
```

### Get the next task

```bash
conda run -n desloppify desloppify next
```

### Rescan (only when queue is clear)

```bash
conda run -n desloppify desloppify scan
```

### Resolve a completed cluster

```bash
conda run -n desloppify desloppify plan resolve "<cluster-name>" \
  --note "what you did" --confirm
```

### Skip a false positive or wontfix item

```bash
conda run -n desloppify desloppify plan skip "<issue-id>" --permanent \
  --note "reason" \
  --attest "I have reviewed this skip against the code and I am not gaming the score. <detail>."
```

Attestation must contain "not gaming" and either "reviewed" or "i have actually".

### Subjective review workflow

1. Generate batch prompts (dry run — no subagents launched):

   ```bash
   conda run -n desloppify desloppify review --run-batches --dry-run
   ```

   Prompt files land in `.desloppify/subagents/runs/<timestamp>/prompts/`.

2. Launch subagents in groups of **4–5** (never all at once — hits session limits).
   Each reads `prompts/batch-N.md` and writes JSON to `results/batch-N.raw.txt`.
   Prompt to give each subagent:

   > "Read the full prompt from `<path>/batch-N.md`, follow ALL instructions exactly,
   > and write your JSON output to the corresponding results file."

3. Import results and rescan:

   ```bash
   conda run -n desloppify desloppify review \
     --import-run .desloppify/subagents/runs/<timestamp> --scan-after-import
   ```

4. Run triage:

   ```bash
   conda run -n desloppify desloppify plan triage
   ```

   Stages: strategize → observe → reflect → organize → enrich → sense-check → write strategy.
   Each stage requires citing specific issue IDs with `review::` prefix.

5. Record commits after fixing:

   ```bash
   conda run -n desloppify desloppify plan commit-log record
   ```
