# Extra notes for posterity

Last updated: 2026-02-01 17:37:43Z

## Why “upload file → agent generates patch” became the preferred workflow

- Patch apply failures (‘patch does not apply’) were repeatedly triggered by file drift.

- Locally executed scripts risked silent corruption (e.g., literal `\t` insertion).

- Uploading exact working-tree file bytes enabled reliable patch regeneration.

## Behavioural nuance

- Logs like “refusing to write entry … because disk would exceed limit” indicate the **PersistentCache disk cap** enforcement, not necessarily a filesystem full condition.

## Test hygiene

- For missing-shard tests:

  - select a shard that actually exists

  - restore to the exact original basename

  - prefer a fresh cache directory to avoid unrelated cap/corruption noise
