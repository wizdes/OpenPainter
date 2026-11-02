# Review of the Windows plan, revision 2

A fresh native Opus reviewer read revision 2 on 2026-11-01, read-only, in about 11 minutes. Astra did not review it: the slot stayed full for 23 tries over 45 minutes. Root retries at the end of Phase A.

Root checked findings 5, 7, 9, 10, 11, 13, 16, 20 and the manifest finding against the code or the machine. All 23 are accepted, and the fixes are in revision 3 of `PLAN.md` and in `plans/W1.md`.

| # | Finding | Fix |
| --- | --- | --- |
| 1 | W5 could close with most of its evidence NOT RUN | W5 stops for decision 4. Launch budget defined. |
| 2 | W4, W14, W15 need the user's paint.net closed, and no decision asked | Decision 4 widened. Preflight check and folder backup added. |
| 3 | W1's Mandelbrot test could make its done-test impossible | Third exception, and the user's list. |
| 4 | Choosing WinUI breaks the test plan | Re-plan step for W7 to W18. |
| 5 | `m54_parity.rs:702` silently skips `pdn-5.1` rows | W4 reads those rows. A row with no capture never passes. |
| 6 | Done-tests named Mac checks that cannot run then | Rewritten as NOT RUN and owed. Root asks for the Mac session. |
| 7 | `recovery.rs:618` blocks every new record; W2 never had to clear W1's list | W2's done-test clears the list. |
| 8 | A reader holding a file breaks close-before-rename | W2 avoids the rename, with a test. |
| 9 | W1's folder sync recipe fails, and it missed the real save paths | Write handle stated. All 3 files named. Replace test. |
| 10 | Real `file:///C:/` URLs make "recover over a newer file" pass | W1 row and tests. |
| 11 | The GPU reset test only sets a flag | W3 tries a real device removal. |
| 12 | W5's IME and pen tests skip the hard case | Tested on the canvas. Focus rules in W8. |
| 13 | W13's input methods change the user's settings | Decision 8. |
| 14 | Three done-tests pass by giving up | W11, W14, W15 limits. |
| 15 | Late dependencies wrong; W7 opens a PNG before Open exists; "60 fps" undefined | Tables fixed. Test hook. M12's definition. |
| 16 | `opcli dump-catalog` does not exist | W7 builds it and the generator. |
| 17 | Phase B budgets well below the Mac's planned hours | Raised. Total about 410 h. Still below the Mac, because the engine exists. |
| 18 | Reviews further apart than the workspace rule | W1, W2, W10 get reviews. Listed as a choice to check. |
| 19 | Release items did not fit together | W10 and W19 rows. |
| 20 | `low_disk` cannot run as `agent` | Reports NOT RUN. W19 covers low disk. |
| 21 | Hidden desktop may share the user's clipboard | Section 4.4 rule. |
| 22 | Color can pass while the screen is wrong | W3 covers DWM color management. |
| 23 | Private captures clash with the NOT RUN rule | Section 4.1. |

Smaller items: the backslash in manifest names (W1), who builds W4's plugin (section 3.2), the W10 codec table (W10 row), the old lock layout (W2 row), and the banned-name scan (W19 row) are in. A test rewrite for the lock layout, if W2 needs one, asks the user first.

Uncertain items go to probes: AOT with the headless test framework (W5), private types in `CreateInstance` (W4), lock release lag after `TerminateProcess` (W2), Avalonia's native DLLs against the 75 MB limit (W19), and WARP speed for the GPU matrix (W3). Whether matching effects at ±1/255 raises a legal question stays with the user at W19; it is flagged in the risks.
