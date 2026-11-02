# W1 Windows engine test counts

Source counts are literal `#[test]` attributes. Windows counts are tests that finished (passed or failed), including the two focused manual runs. Ignored tests are listed separately. A property-test macro can make the Windows count larger than the literal count. Mac runtime counts: **NOT RUN**.

The measured files contain **114** literal test-bearing Rust files and **687** literal test attributes. **623** Windows tests finished across the logged checks; **7** normal-run tests were ignored. The final non-GPU workspace run had **29** failed tests. The manual huge-document and GPU color checks add two failures, for **31** observed failed tests.

## Counts by source file

| File under engine/ | Source `#[test]` | Windows ran | Why the counts differ |
| --- | ---: | ---: | --- |
| `op-cli/src/scenario.rs` | 2 | 2 |  |
| `op-cli/tests/recovery_kill.rs` | 2 | 2 |  |
| `op-cli/tests/scenario.rs` | 1 | 1 |  |
| `op-color/src/hash.rs` | 1 | 1 |  |
| `op-color/tests/accuracy.rs` | 9 | 9 |  |
| `op-color/tests/display_table.rs` | 3 | 3 |  |
| `op-color/tests/icc_nested_mluc.rs` | 1 | 1 |  |
| `op-color/tests/linkage.rs` | 1 | 1 |  |
| `op-core/src/tiles.rs` | 1 | 1 |  |
| `op-core/tests/history.rs` | 10 | 17 | A proptest macro generates seven tests beyond the ten literal attributes. |
| `op-core/tests/history_replace.rs` | 1 | 1 |  |
| `op-core/tests/pixel_legacy.rs` | 5 | 5 |  |
| `op-core/tests/selection.rs` | 7 | 7 |  |
| `op-core/tests/tiles.rs` | 36 | 36 |  |
| `op-engine/src/color_sample.rs` | 2 | 2 |  |
| `op-engine/src/gpu_effects.rs` | 5 | 5 |  |
| `op-engine/src/image_commands.rs` | 1 | 1 |  |
| `op-engine/src/recovery.rs` | 8 | 8 |  |
| `op-engine/src/text_commands.rs` | 1 | 1 |  |
| `op-engine/tests/brush.rs` | 1 | 1 |  |
| `op-engine/tests/clipboard.rs` | 12 | 12 |  |
| `op-engine/tests/clone_recolor.rs` | 2 | 2 |  |
| `op-engine/tests/color_sample.rs` | 4 | 4 |  |
| `op-engine/tests/doc_profile.rs` | 1 | 1 |  |
| `op-engine/tests/document_file.rs` | 7 | 7 |  |
| `op-engine/tests/engine.rs` | 11 | 11 |  |
| `op-engine/tests/fix_round2.rs` | 12 | 12 |  |
| `op-engine/tests/fix_round3.rs` | 8 | 8 |  |
| `op-engine/tests/flood.rs` | 2 | 2 |  |
| `op-engine/tests/flood_session.rs` | 4 | 4 |  |
| `op-engine/tests/gpu_effects.rs` | 11 | 11 |  |
| `op-engine/tests/gradient.rs` | 1 | 1 |  |
| `op-engine/tests/heal.rs` | 2 | 2 |  |
| `op-engine/tests/history_stress.rs` | 1 | 1 |  |
| `op-engine/tests/huge_document.rs` | 1 | 1 | Ignored in the normal suite; its manual run failed at recovery setup (W2). |
| `op-engine/tests/image_menu.rs` | 11 | 11 |  |
| `op-engine/tests/jobs.rs` | 12 | 12 |  |
| `op-engine/tests/layer_menu.rs` | 4 | 4 |  |
| `op-engine/tests/layers_panel.rs` | 8 | 8 |  |
| `op-engine/tests/line.rs` | 4 | 4 |  |
| `op-engine/tests/live_stroke.rs` | 3 | 3 |  |
| `op-engine/tests/low_disk.rs` | 1 | 0 | The hdiutil disk-image test is macOS only; no safe Windows mount as standard user. |
| `op-engine/tests/m54_parity.rs` | 12 | 10 | Two existing release sweep and speed tests are ignored. |
| `op-engine/tests/menu_labels.rs` | 2 | 2 |  |
| `op-engine/tests/move.rs` | 5 | 5 |  |
| `op-engine/tests/profile_dialog.rs` | 2 | 2 |  |
| `op-engine/tests/recovery.rs` | 10 | 10 |  |
| `op-engine/tests/recovery_fixes.rs` | 7 | 6 | One Unix mode-bit permission test is gated; Windows ACL failure injection waits for W2. |
| `op-engine/tests/recovery_fixes2.rs` | 6 | 6 |  |
| `op-engine/tests/release_review.rs` | 10 | 10 |  |
| `op-engine/tests/remove_background.rs` | 7 | 7 |  |
| `op-engine/tests/save_export.rs` | 5 | 5 |  |
| `op-engine/tests/select_crop.rs` | 10 | 10 |  |
| `op-engine/tests/selection_tools.rs` | 21 | 21 |  |
| `op-engine/tests/shapes.rs` | 1 | 1 |  |
| `op-engine/tests/shell_commands.rs` | 7 | 7 |  |
| `op-engine/tests/text.rs` | 5 | 5 |  |
| `op-ffi/src/recovery.rs` | 1 | 1 |  |
| `op-ffi/tests/document.rs` | 1 | 1 |  |
| `op-ffi/tests/ffi.rs` | 3 | 3 |  |
| `op-ffi/tests/flood_session.rs` | 1 | 1 |  |
| `op-ffi/tests/jobs.rs` | 4 | 4 |  |
| `op-ffi/tests/live_stroke.rs` | 2 | 0 | Two CAMetalLayer view tests are macOS only; Windows view host waits for W3. |
| `op-ffi/tests/move_tool.rs` | 4 | 4 |  |
| `op-ffi/tests/perf.rs` | 1 | 1 |  |
| `op-ffi/tests/png_file_limit.rs` | 1 | 1 |  |
| `op-ffi/tests/remove_background.rs` | 1 | 1 |  |
| `op-ffi/tests/views.rs` | 7 | 0 | Seven CAMetalLayer view tests are macOS only; Windows view host waits for W3. |
| `op-formats/src/opaint.rs` | 6 | 6 |  |
| `op-formats/tests/cmyk.rs` | 3 | 3 |  |
| `op-formats/tests/corpus.rs` | 7 | 7 |  |
| `op-formats/tests/io.rs` | 17 | 17 |  |
| `op-formats/tests/opaint.rs` | 9 | 9 |  |
| `op-formats/tests/opaint_limits.rs` | 12 | 12 |  |
| `op-formats/tests/png.rs` | 6 | 6 |  |
| `op-formats/tests/png16.rs` | 2 | 2 |  |
| `op-formats/tests/robustness.rs` | 3 | 3 |  |
| `op-formats/tests/save_options.rs` | 5 | 5 |  |
| `op-formats/tests/tiff_exif.rs` | 5 | 5 |  |
| `op-formats/tests/webp_memory.rs` | 1 | 1 |  |
| `op-gpu/tests/canvas.rs` | 5 | 5 |  |
| `op-gpu/tests/effects.rs` | 23 | 8 | Eight finished; one existing timing test ignored; 14 active tests were interrupted in the W3 GPU run. |
| `op-gpu/tests/gaussian_cached.rs` | 1 | 0 | The op-gpu run stopped during effects before this file (W3). |
| `op-gpu/tests/lazy_pipelines.rs` | 1 | 0 | The op-gpu run stopped during effects before this file (W3). |
| `op-gpu/tests/parity.rs` | 10 | 0 | The op-gpu run stopped during effects before this file (W3). |
| `op-gpu/tests/presenter.rs` | 4 | 1 | Only the focused color-table test ran and failed; three tests were not reached (W3). |
| `op-gpu/tests/region_upload.rs` | 1 | 0 | The op-gpu run stopped during effects before this file (W3). |
| `op-gpu/tests/resample.rs` | 5 | 0 | The op-gpu run stopped during effects before this file (W3). |
| `op-gpu/tests/roundtrip.rs` | 4 | 0 | The op-gpu run stopped during effects before this file (W3). |
| `op-ops/src/effects/curves.rs` | 1 | 1 |  |
| `op-ops/tests/blurs_m48.rs` | 5 | 5 |  |
| `op-ops/tests/composite.rs` | 8 | 8 |  |
| `op-ops/tests/content_aware.rs` | 9 | 8 | One existing release speed test is ignored. |
| `op-ops/tests/distort_m45.rs` | 6 | 6 |  |
| `op-ops/tests/effects.rs` | 39 | 38 | One existing 24 MP CPU timing test is ignored. |
| `op-ops/tests/image.rs` | 6 | 6 |  |
| `op-ops/tests/m40.rs` | 6 | 6 |  |
| `op-ops/tests/m49.rs` | 5 | 5 |  |
| `op-ops/tests/render_m47.rs` | 5 | 5 |  |
| `op-present-spike/src/lib.rs` | 3 | 0 | Three macOS Metal spike tests; W3 adds a Windows view host. |
| `op-testkit/src/lib.rs` | 8 | 8 |  |
| `op-tools/src/heal.rs` | 1 | 1 |  |
| `op-tools/tests/brush.rs` | 12 | 12 |  |
| `op-tools/tests/clone_recolor.rs` | 6 | 6 |  |
| `op-tools/tests/flood.rs` | 10 | 9 | One existing large timing test is ignored. |
| `op-tools/tests/gradient.rs` | 7 | 7 |  |
| `op-tools/tests/heal.rs` | 5 | 5 |  |
| `op-tools/tests/line.rs` | 5 | 5 |  |
| `op-tools/tests/move.rs` | 6 | 6 |  |
| `op-tools/tests/pen_input.rs` | 8 | 8 |  |
| `op-tools/tests/shapes.rs` | 4 | 4 |  |
| `op-tools/tests/text.rs` | 9 | 9 |  |
| `xtask/src/fuzz.rs` | 6 | 5 | One SIGKILL test is Unix only; Windows has two process-watchdog tests. |
| `xtask/src/main.rs` | 21 | 10 | Eleven Bash or macOS app-script tests are gated to Unix or macOS. |

## Tests still failing

The line is where each test failed. W2 owns the recovery folder change: Windows refuses to rename a folder with its open `owner.lock` inside. Mandelbrot goes to the user's list, because its cause is not settled and a fix may change the Mac's output. W14 cannot close without that answer. W3 owns the GPU color table.

| Failing line under engine/ | Test | Reason | Waits for |
| --- | --- | --- | --- |
| `op-cli/tests/recovery_kill.rs:55:17` | `the_latest_acknowledged_revision_recovers_whenever_the_process_is_killed` | Recovery cannot create or rename a record folder while its lock file is open. | W2 |
| `op-cli/tests/recovery_kill.rs:156:5` | `dont_save_then_a_fast_quit_leaves_nothing_to_recover` | Recovery cannot create or rename a record folder while its lock file is open. | W2 |
| `op-engine/src/recovery.rs:1592:18` | `recovery::tests::a_new_folder_is_never_visible_without_its_lock_held` | Recovery cannot create or rename a record folder while its lock file is open. | W2 |
| `op-engine/tests/fix_round2.rs:333:6` | `a_recovery_record_keeps_the_original_content_type` | Recovery cannot create or rename a record folder while its lock file is open. | W2 |
| `op-engine/tests/m54_parity.rs:727:5` | `m54_legacy_rows_hold_on_small_fixtures_through_the_tiled_path` | Mandelbrot output differs from the legacy reference by 23 of 255 levels. | W14 |
| `op-engine/tests/recovery.rs:336:5` | `a_folder_without_a_manifest_is_cleaned_up_by_scan` | Recovery cannot create or rename a record folder while its lock file is open. | W2 |
| `op-engine/tests/recovery.rs:132:48` | `a_clean_save_and_undo_to_saved_delete_the_record_and_close_deletes_the_folder` | Recovery cannot create or rename a record folder while its lock file is open. | W2 |
| `op-engine/tests/recovery.rs:301:48` | `a_damaged_snapshot_is_listed_as_unusable` | Recovery cannot create or rename a record folder while its lock file is open. | W2 |
| `op-engine/tests/recovery.rs:171:10` | `a_crashed_documents_record_is_found_and_recovers_unsaved_at_its_revision` | Recovery cannot create or rename a record folder while its lock file is open. | W2 |
| `op-engine/tests/recovery.rs:60:10` | `a_record_is_written_after_edits_settle_and_names_the_revision` | Recovery cannot create or rename a record folder while its lock file is open. | W2 |
| `op-engine/tests/recovery.rs:363:49` | `a_record_write_never_blocks_commands_or_queries` | Recovery cannot create or rename a record folder while its lock file is open. | W2 |
| `op-engine/tests/recovery.rs:118:49` | `settle_delay_holds_back_a_write_while_edits_continue` | Recovery cannot create or rename a record folder while its lock file is open. | W2 |
| `op-engine/tests/recovery.rs:241:48` | `a_failed_record_write_keeps_the_previous_record_and_reports_the_error` | Recovery cannot create or rename a record folder while its lock file is open. | W2 |
| `op-engine/tests/recovery.rs:97:49` | `continuous_edits_still_write_within_the_max_interval` | Recovery cannot create or rename a record folder while its lock file is open. | W2 |
| `op-engine/tests/recovery.rs:220:48` | `discard_deletes_a_crashed_record` | Recovery cannot create or rename a record folder while its lock file is open. | W2 |
| `op-engine/tests/recovery_fixes.rs:153:49` | `item2a_a_clean_save_during_a_record_write_never_leaves_the_older_record` | Recovery cannot create or rename a record folder while its lock file is open. | W2 |
| `op-engine/tests/recovery_fixes.rs:104:49` | `item1_a_failed_folder_sync_after_the_manifest_rename_keeps_the_new_record` | Recovery cannot create or rename a record folder while its lock file is open. | W2 |
| `op-engine/tests/recovery_fixes.rs:330:49` | `item6_close_during_a_record_write_returns_at_once` | Recovery cannot create or rename a record folder while its lock file is open. | W2 |
| `op-engine/tests/recovery_fixes.rs:365:49` | `item6_a_stop_during_a_write_keeps_the_last_good_record` | Recovery cannot create or rename a record folder while its lock file is open. | W2 |
| `op-engine/tests/recovery_fixes.rs:184:10` | `item2b_a_record_older_than_its_file_is_not_offered` | Recovery cannot create or rename a record folder while its lock file is open. | W2 |
| `op-engine/tests/recovery_fixes.rs:253:5` | `item3_concurrent_attaches_and_scans_lose_no_folder` | Recovery cannot create or rename a record folder while its lock file is open. | W2 |
| `op-engine/tests/recovery_fixes2.rs:222:49` | `item4_a_save_does_not_wait_for_a_record_write` | Recovery cannot create or rename a record folder while its lock file is open. | W2 |
| `op-engine/tests/recovery_fixes2.rs:285:49` | `round3_the_acknowledged_revision_is_cleared_before_the_record_goes` | Recovery cannot create or rename a record folder while its lock file is open. | W2 |
| `op-engine/tests/recovery_fixes2.rs:97:49` | `item1_close_removes_the_folder_before_it_returns` | Recovery cannot create or rename a record folder while its lock file is open. | W2 |
| `op-engine/tests/recovery_fixes2.rs:187:6` | `item3_an_edit_and_save_after_the_state_load_is_not_committed_as_older` | Recovery cannot create or rename a record folder while its lock file is open. | W2 |
| `op-engine/tests/recovery_fixes2.rs:139:6` | `item2_a_record_older_than_its_file_recovers_as_untitled` | Recovery cannot create or rename a record folder while its lock file is open. | W2 |
| `op-engine/tests/release_review.rs:173:56` | `the_recovery_record_holds_the_open_text` | Recovery cannot create or rename a record folder while its lock file is open. | W2 |
| `op-engine/tests/release_review.rs:354:10` | `kept_image_metadata_survives_opaint_and_recovery` | Recovery cannot create or rename a record folder while its lock file is open. | W2 |
| `op-ops/tests/render_m47.rs:346:13` | `legacy_render_oracle_every_fixture_and_variant` | Mandelbrot output differs from the legacy reference by 23 of 255 levels. | W14 |
| `op-engine/tests/huge_document.rs:136` | `a_200_megapixel_document_paints_blurs_undoes_and_saves_under_the_memory_cap` | Recovery attach fails before the later memory check. | W2 |
| `op-gpu/tests/presenter.rs:182` | `lut_on_gpu_matches_cpu_trilinear_within_one_level` | Vulkan GPU color-table error is 0.55805206 levels. | W3 |

## What changed

- `op-core/src/tiles.rs`: spill reads and writes use `seek_read` and `seek_write` with an explicit offset on one shared handle. These move the handle's cursor, but nothing depends on the cursor. The test reads from 8 threads.
- `op-formats/src/lib.rs`, `op-formats/src/opaint.rs`, `op-engine/src/document_file.rs`, and `op-engine/src/recovery.rs`: a save copies the old file's access rules, extra streams, creation time and hidden and system attributes onto the temporary file, then swaps them in one atomic rename. `ReplaceFileW` was tried and dropped: a review proved a gap where no file has the name, and a killed process then lost a recovery record. Recovery manifests use the plain rename. A read-only target or a folder is refused at once. A held target is retried for about 2 s, and then the error names it. A failed flush after the rename is returned, as on Unix, except on a drive with no folder flush.
- `op-engine/src/recovery.rs`: Windows URL decoding now keeps drive letters, Unicode and UNC paths. A save past the old 260-character limit passes. A record whose original file cannot be checked is now offered as possibly older, while a file that is really gone stays safe. Manifests now reject Windows backslashes and colons in snapshot names. File identity uses the volume and file index from `GetFileInformationByHandle`.
- `op-color/src/lib.rs`: lcms2 panics on a wide character it cannot convert. On Windows that includes a valid surrogate pair, because `wchar_t` is 16 bits there. Only the name falls back to "Embedded ICC profile", and the rest of the profile is still used.
- `xtask/src/fuzz.rs` and `op-formats/examples/common/capped_alloc.rs`: the Windows fuzz watchdog reads a process's working set and uses `TerminateProcess`. The worker reports that Windows has no `setrlimit`; its Rust allocation cap and the driver watchdog stay active.
- `op-ffi`: `cargo rustc -p op-ffi --lib --crate-type cdylib` builds `op_ffi.dll`, so the Mac's crate types stay as they were. Python `ctypes` found `op_engine_new`, `op_engine_free` and `op_import_pixels` in it. W7 adds a test.
- `op-formats/tests/opaint_limits.rs`, `op-engine/tests/release_review.rs`, `op-engine/tests/huge_document.rs`, `op-engine/tests/recovery_fixes.rs`, `op-color/tests/accuracy.rs`, plus the inline tests above: Windows symlinks, working-set memory and saved regressions. The Unix mode-bit permission test remains Unix only because Windows readonly flags do not deny file deletion the same way.
- `engine/Cargo.lock`, `op-engine/Cargo.toml`, `op-formats/Cargo.toml`, and `xtask/Cargo.toml`: Windows API dependency records and features.

## Mandelbrot finding

The first failing reference pixel is `(16, 6)` in `Mandelbrot_Fractal_default_impulse_64.png`. The test at `op-ops/tests/render_m47.rs:346` reads `[0, 1, 79, 251]` from the reference and gets `[0, 24, 79, 251]` here. The default angle is zero, but `fractal_common.rs` still computes `atan2`, `cos` and `sin` to rotate each sample by zero. In a calculation using that file's equations, sample 1 escapes after 571 iterations through the trig path and after 456 when its original `(u, v)` is used. The two paths differ by about one `f64` unit in the last place before iteration. Averaging the five samples gives the exact two pixels above. This proves that a tiny trig-rounding change is amplified by the Mandelbrot iteration. It does **not** prove that a particular Mac or Windows math library caused the initial rounding. No effect output changed to make the golden pass. W14 must compare the new reference; a Mac run must check any later output change.

## Checks and limits

| Exact command from `engine/` | Result |
| --- | --- |
| `cargo test --workspace --all-targets --no-fail-fast` | First logged run in `target/w1-full.log`; stopped during the old GPU run after it exposed the cursor, ICC and save failures. |
| `cargo test -p op-core --lib positioned_spill_reads_work_at_any_offset_across_threads -- --nocapture` | Passed 1/1 after the same saved test failed in the first workspace log. |
| `cargo test -p op-engine --lib windows_directory_sync_accepts_a_real_directory -- --nocapture` | Failed with error 5 before the Windows folder path changed. |
| `cargo test -p op-engine --lib windows_manifest_rejects_backslash_path_escape -- --nocapture` | Failed before the name check changed. `cargo test -p op-engine --lib windows_ -- --nocapture` then passed 6/6, including drive, UNC, Unicode and long-path tests. |
| `cargo test -p op-color --test accuracy damaged_icc_description_returns_an_error_instead_of_panicking -- --nocapture` | With the guard temporarily removed, failed at lcms2 `profile.rs:307`; after restoration, passed 1/1. Logs: `target/w1-icc-specific-before.log` and `target/w1-icc-specific-after.log`. |
| `cargo test -p op-formats --test opaint_limits windows_saves_replace_an_existing_opaint_file -- --nocapture` | With the Windows save call temporarily set back to folder sync, failed with error 5; after restoration, passed 1/1. Logs: `target/w1-save-specific-before.log` and `target/w1-save-specific-after.log`. |
| `cargo test -p op-formats --lib a_disk_full_save_keeps_the_old_file_and_reports_enospc -- --nocapture` | Failed in `target/w1-nongpu.log` because Windows words error 28 differently; passed 1/1 after the Windows message assertion was ported. The Unix assertion stays as it was. |
| `cargo test -p op-formats --test opaint_limits --no-fail-fast -- --nocapture` | Passed 12/12. |
| `cargo test -p op-engine --test document_file --no-fail-fast` | Passed 7/7. |
| `cargo test -p op-color --test accuracy --no-fail-fast` | Passed 9/9. |
| `cargo test -p xtask --no-fail-fast` | Passed 15/15. |
| `cargo build -p op-formats --example fuzz_worker` and `cargo build -p op-engine --example fuzz_engine_worker` | Both passed. |
| `cargo run -p xtask -- fuzz --seconds 1 icc` | 67,545 cases; zero crashes, hangs, aborts or out-of-memory cases. Log: `target/w1-fuzz.log`. |
| `cargo rustc -p op-ffi --lib --crate-type cdylib` | Passed; the DLL loads through `ctypes`, and 3 named exports were found. |
| `cargo test --workspace --no-fail-fast` | Stopped during `op-gpu/tests/effects.rs` after 14 tests remained active for several minutes. Log: `target/w1-final.log`. **Full GPU suite NOT COMPLETED.** |
| `cargo test --workspace --exclude op-gpu --no-fail-fast` | Final log `target/w1-nongpu-final.log`: 29 failed tests in nine test targets, all listed above; other test targets finished. |
| `cargo test -p op-engine --test huge_document --no-fail-fast -- --ignored --nocapture` | Failed at `huge_document.rs:136` during W2 recovery attach. The later memory-cap assertion was **NOT RUN**. Log: `target/w1-huge.log`. |
| `cargo test -p op-gpu --test presenter lut_on_gpu_matches_cpu_trilinear_within_one_level -- --test-threads=1 --nocapture` | Failed at `presenter.rs:182`, worst 0.55805206 levels; adapter was Vulkan, not DX12. Log: `target/w1-gpu-color.log`. |
| `cargo check --workspace --all-targets -q` and `cargo fmt --all --check` | Both passed on Windows. |
| `cargo check --workspace --target aarch64-apple-darwin` | Failed with `can't find crate for core`. `rustup target list --installed` shows only `x86_64-pc-windows-msvc`. This is a missing target, before any C dependency check. **Apple compile NOT RUN.** |

Two earlier failures were intermittent. `op-engine/tests/jobs.rs:461` ended twice with Windows `STATUS_ACCESS_VIOLATION` when GPU creation was on; the same test passed with `OP_FORCE_CPU=1` and in the final workspace run. `op-ffi/tests/jobs.rs:133` timed out waiting five seconds for ticket 102 in the first non-GPU run, then passed alone and in the final run. These passes do not erase the failures. W3 should pin and retest the GPU backend; W19 should check job timing under load.

**NOT RUN:** Mac and Linux runtime suites; an actual UNC share; the macOS-only `low_disk` disk-image test on Windows; the W3 Windows view host; the remaining `op-gpu` test files after the interrupted run; a fresh independent review (the builder brief forbids other models and agents). The Windows `sync_dir` branch checks that a directory exists. A backup-semantics handle followed by `sync_all` returned error 5 here. The write-through file rename covers committed file replacement, but a crash-durability proof for recovery folder creation and deletion remains **NOT RUN** and belongs with W2's folder redesign.

## One-line follow-ups

- W2: move the lock out of the record folder, close it before folder moves, then rerun every recovery and huge-document test.
- W3: pin DX12 or WARP, finish the GPU suite, and retest the color table and GPU-enabled job crash.
- User's list: Mandelbrot differs by up to 23 of 255 levels. Decide once a Mac run shows whether the Mac still matches the reference.
- W2: port `item5_a_failed_clean_up_is_reported_and_retried` in `recovery_fixes.rs`, which has no Windows version yet. `tests/recovery.rs:259`, `:261` and `:293` assert the Unix text "No space left" and need the Windows text once W2 lets them run.
- W7: build the DLL with `cargo rustc -p op-ffi --lib --crate-type cdylib`, so the Mac's crate types stay as they were, and test its exports.
- Fuzz: `rss_kib` keeps returning a value while the child's handle is open, so the Windows watchdog polls until the handle closes.
- W19: stress the C-interface job queue under load.
- Tool setup: install the missing Apple target outside this builder's owned paths, then rerun the cross-target check and Mac runtime tests.
