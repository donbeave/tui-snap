# CI wiring

Rules: gates run everywhere; approval happens only on a human workstation.
CI must never accept snapshots by itself — there is no flag or variable
that approves, so the only way to break this rule is to run `accept` in CI.
Don't.

```yaml
- name: Visual gates
  run: cargo test --locked

- name: Publish visual evidence (on failure too)
  if: always()
  uses: actions/upload-artifact@v4
  with:
    name: visual-evidence
    path: |
      tests/visual/actual/
      tests/visual/diff/
      tests/visual/report.html
```

Notes:

- Determinism: same frame + same profile + same vendored font bytes =
  byte-identical PNGs (tested). Runners need no system fonts.
- `report` subcommand regenerates the index without approving anything:
  `tuisnap report --store tests/visual` exits nonzero on mismatch.
- Review flow for a red run: download `visual-evidence`, open
  `report.html` (expected/actual/diff + cell table + embedded frame JSON),
  reproduce locally if needed, then `tuisnap accept` locally and push the
  updated `approved/*.frame.json`.
- Parallel jobs are safe: per-name files + atomic renames. Concurrent jobs
  sharing one store directory only race on `report.html` (best-effort
  index); gate data never clobbers.
- Do not cache `actual/`, `diff/`, or `report.html` between runs — they are
  per-run evidence. Approved PNGs need not be committed (deterministic
  regeneration); only `approved/*.frame.json` is versioned.
