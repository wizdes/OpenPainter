# Review of the Windows plan, revision 1

A fresh native Opus reviewer read revision 1 on 2026-11-01. It ran read-only and took about 12 minutes. Astra did not review it, because root on this PC cannot reach the `agent` account. Run Astra on revision 2 once decision 1 is settled.

Root checked findings 1, 3, 4, 9 and 15 against the code or by reading the reviewer's evidence. All 26 are accepted. The fix is in `PLAN.md`.

| # | Finding | Fix |
| --- | --- | --- |
| 1 | Avalonia cannot draw over a native child window, and the Mac's overlays sit over the canvas | Section 3.1 states it. W5 must prove overlays. Confidence lowered to medium. |
| 2 | Real-window tests had no fallback, and builder code would run as `yjli` | Section 4.4 and the NOT RUN rule. Only root runs these, on merged code. |
| 3 | Windows refuses to rename a folder while a file in it is open (reviewer ran this) | New W2. |
| 4 | W1 depended on W2, and "the GPU really ran" would pass on Vulkan | W1 lists GPU tests as waiting. W3 pins the backend and asserts it. |
| 5 | Whole test files are Mac-only, so "0 new skips" cannot see them | W1 compares per-file counts with the Mac. |
| 6 | W2's resize tests are Swift only, and no color method existed | W3 has a Win32 host and an lcms2 color reference. |
| 7 | Missing dependencies across 8 milestones | Tables and order redrawn. Files move after core tools. |
| 8 | W3 proved too little and did not pin GPU or driver | W4 tests one effect per family and pins both. |
| 9 | 30 h was far too low, and "written reason" let any effect pass | W14 has an estimate, a report first, and ±1/255 or the user's list. |
| 10 | 45 minutes cannot fit the full run; `op-check` was never built | Levels extend `cargo xtask check`, about 2 h in parts, built in W0. |
| 11 | The native Opus fallback runs beside `~/keys`, and the runner refuses Claude on Windows | Section 6 says wait, or root builds. |
| 12 | A new account on this PC proves little; no DLL scan; deploy from this PC is banned | W19 has a DLL scan and decision 5. The page deploys from a Mac session. |
| 13 | SF Symbols cannot ship on Windows, and no milestone replaced them | W6 adds an icon set. Apple's terms were not re-read; replacement is needed either way. |
| 14 | Text works on the Mac and has its own gates; the 7 tools need new C paths | W12 and W13 are separate, with more hours. |
| 15 | .NET 9 support ends 2026-11-10 | W0 installs .NET 10. |
| 16 | "Never copy strings" conflicts with the catalog; no name scan | Section 3.2 rewritten. W19 has the scan. Model license named. |
| 17 | No limits on CPU, GPU, Narrator, sleep or scaling tests | Section 4.4 and decision 4. |
| 18 | One screen, no pen, long paths already on | Marked NOT RUN, or the setting is recorded. |
| 19 | Mac features with no milestone | Added to W7, W8, W10, W11 and W17. |
| 20 | File association, second-file open, DLL link, antivirus retries, local app data | Added to W1 and W10. |
| 21 | Mac and Linux could break unseen | W3 and W2 check the Mac. New W20. |
| 22 | The private repo has access gaps | Root captures and copies images in. Decision 6. |
| 23 | W13 clashed with "never capture pixels" for tools | Section 3.2. `parity.md` starts in W4. |
| 24 | W4's change conditions were too narrow; tests ran under JIT only | W5 conditions widened. The milestone level runs on the AOT build. |
| 25 | Workspace rule details | Choices to check, review deadlines, and split decision 1 into 1 and 2. |
| 26 | Small errors | Fixed. |

Uncertain items 1 to 7 go to the W0, W3 or W5 probes. Item 8 is decision 7. Item 9 is in the risks. Item 10 is a choice to check.
