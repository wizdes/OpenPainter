# Windows build state

Branch `windows`, worktree `Projects/OpenPainter-windows`. Plan: `PLAN.md`.

- [x] W0: machine and test setup, closed 2026-11-01. Results: `probes/W0.md`.
  - Sol wrote the first pass. The runner marked it failed because cargo output counted as outside edits, which came from root's launch flags. Root read the code and fixed the preflight's Sunshine check and the hidden desktop probe itself, since Opus cannot run on Windows.
  - Second paint.net copy check NOT RUN. Captures assume the user's copy is closed.
- [ ] W1: engine files and tests
- [ ] W2 to W20: see `PLAN.md`

## Open items

- Decision 2 (keys for Mac and Linux), decision 4 (window hours), decision 5 (Store login, at W19).
- Astra's review of plan revision 2.
