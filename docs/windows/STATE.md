# Windows build state

Branch `windows`, worktree `Projects/OpenPainter-windows`. Plan: `PLAN.md`.

- [x] W0: machine and test setup, closed 2026-11-01. Results: `probes/W0.md`.
  - Sol wrote the first pass. The runner marked it failed because cargo output counted as outside edits, which came from root's launch flags. Root read the code and fixed the preflight's Sunshine check and the hidden desktop probe itself, since Opus cannot run on Windows.
  - Second paint.net copy check done later: it hands off, so captures need the user's copy closed.
- [ ] W1: engine files and tests
- [ ] W4: capture kit, building beside W1
- [ ] W2, W3, W5 to W20: see `PLAN.md`

## Open items

- This PC's key on the Macs (the user must add it). The Linux box is done. Decision 5 waits for W19.
- Astra's review of plan revision 2.
