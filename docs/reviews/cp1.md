# CP1 review (M01, M03, M04, M05)

Two independent read-only reviews ran on commit 9d8cace: Astra (through the runner) and a fresh Opus reviewer. Root checked each finding against the code. Fixes went to one Opus fix round (`op-cp1-opus-fix`).

## Checks root ran
- `cargo xtask check --tier checkpoint` on the Mac: passed.
- Linux VM: the first run reported a pass but tested old code. Cause: xtask fixed the engine path at compile time, and the VM's shared folder view lagged. Fixed in 4ab2628; the rerun passed with the tile tests really running (17 of 17, debug and release).

## Findings and what we did

| # | Finding | Source | Decision |
| --- | --- | --- | --- |
| 1 | The spill file grows without limit: a reloaded spilled tile drops its disk copy, so each evict writes a new record. The file is never deleted, and a leftover name makes `TileStore::new` fail. | Opus | Accepted. Keep the spill record on reload; delete the file on drop; pick a new name on collision. Compaction is a follow-up. |
| 2 | Reading a spilled tile holds the main lock while waiting on disk I/O, so one slow write freezes every caller. | Opus (also the M05 builder's follow-up) | Accepted. Load outside the main lock. |
| 3 | A failed `unpin` returns an error after it already removed the pin. | Both | Accepted. Eviction becomes best effort: it never fails `insert`, `unpin`, `release`, `set_budget` or a guard drop. Failures are counted in stats. |
| 4 | Guard pins and explicit pins share one counter; `get`, `unpin`, then dropping the guard wraps the count. | Opus | Accepted. Separate counters, checked arithmetic. |
| 5 | A failed `set_budget` restores an older budget over a newer concurrent one. | Astra | Accepted, through 3: `set_budget` always keeps the new budget. |
| 6 | A failed eviction can leave earlier rounds' moves published. | Astra | Rejected as a defect. Residency is not content: moving a tile between memory states never changes its bytes. The "no change on failure" rule covers content and counts. Written into the store's docs. |
| 7 | The 8 MiB random test never evicts (at most 2 live tiles); test data compresses about 60 to 1; rollback and publish-guard paths have no tests; one threaded test only. | Both | Accepted. New tests listed in the fix brief. |
| 8 | No tier runs the large random-case counts or the oracle regression tests. | Both | Accepted. Milestone tier: 1,024 cases plus `test_oracle.py`; checkpoint tier: 10,000 cases plus the tile benchmark. |
| 9 | `op-testkit` image validation can overflow `width*height*4` and pass empty images in release builds. | Astra | Accepted. Checked arithmetic. |
| 10 | M03 renders effects on 4 fixtures; the gate says 5. | Astra | Accepted. Add a fifth fixture and regenerate. |
| 11 | Trailing whitespace in the oracle shims fails `git diff --check`. | Astra | Accepted. |
| 12 | A panic during a move leaves tiles marked as moving forever. | Opus (uncertain) | Accepted. Clear marks with an unwind guard. |
| 13 | A failed insert can remove a record another thread already holds, or reuse its id. | Opus (uncertain) | Accepted, through 3: insert no longer rolls back. |
| 14 | `FileIo::append` does not fsync. | Opus (uncertain) | Accepted as a comment fix only: the spill file is a per-session cache. |

## Fix round result (op-cp1-opus-fix)
- Each new tile test failed on the old code; each publish guard has a test that fails when the guard is removed. Tile tests: 17 → 36. 2,000 random cases pass.
- Root reran `tools/pdn-oracle/run-all.sh` outside the sandbox (including the fixtures step the sandbox blocked): every tracked output and all 43 new `alpha_edge_64` images came out byte-identical.
- Two existing tile tests and one M06 history test (`storage_faults_leave_history_unchanged_and_recover`) were changed because finding 3 changed the contract: a failed spill write no longer fails an edit. The new assertions check the new contract strictly (the edit succeeds, the old step is unchanged, `evict_failures` > 0 and `over_budget` while the fault lasts, and it clears after).
- The spill file now uses Unix positioned reads and writes; a Windows version is a follow-up for the Windows port.
- Diagram of the tile states after CP1: `docs/diagrams/cp1-tile-states.html`.

## Follow-ups
- Compact dead spill bytes.
- Windows spill file (`seek_read`/`seek_write`).
- `run-all.sh`: verify instead of rewrite `tests/fixtures/` when it cannot write there.
- Faster debug builds of lz4_flex and crc32fast (history tests take about 2 minutes in debug).
- `TileRef::id()`.
