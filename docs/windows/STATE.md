# Windows build state

Branch `windows`, worktree `Projects/OpenPainter-windows`. Plan: `PLAN.md`.

- [x] W0: machine and test setup, closed 2026-11-01. Results: `probes/W0.md`.
  - Sol wrote the first pass. The runner marked it failed because cargo output counted as outside edits, which came from root's launch flags. Root read the code and fixed the preflight's Sunshine check and the hidden desktop probe itself, since Opus cannot run on Windows.
  - Second paint.net copy check done later: it hands off, so captures need the user's copy closed.
- [x] W1: engine files and tests, closed 2026-11-02. Results: `test-counts.md`.
  - Non-GPU run: 599 passed, 29 failed, 6 ignored, in 9.3 minutes, as `agent` from a fresh clone. All 29 are listed: recovery waits for W2, and Mandelbrot is on the user's list.
  - Astra reviewed twice, and Opus three times. The save design changed twice because of them. The last review found no way to lose data: 210 kills, 0 missing or torn files.
  - Owed to a Mac session: every engine test on the Mac, and the items in `test-counts.md`.
  - An `engine.rs` test run spun at full CPU for over 30 minutes while root ran GPU captures. Alone it passes in 1.62 s. W3 must look at this: suspect GPU contention or a device reset.
  - Tests failed once with `STATUS_ACCESS_VIOLATION` in `op-engine/tests/jobs.rs:461` with GPU creation on. W3 also.
- [ ] W2: recovery redesign
- [ ] W3: GPU and canvas on DX12
- [ ] W4: capture kit, committed at 244f671. Add Noise and Clouds are not repeatable, and parameters cannot be set yet. See `parity.md`. Its review comes at the end of Phase A.
- [ ] W5 to W20: see `PLAN.md`

## Open items

- This PC's key on the Macs (the user must add it). The Linux box is done. Decision 5 waits for W19.
- Astra's review of the plan has not run. W1's code had Astra reviews.
