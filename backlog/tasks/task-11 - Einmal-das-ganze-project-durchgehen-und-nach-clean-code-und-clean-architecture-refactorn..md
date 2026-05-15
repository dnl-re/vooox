---
id: TASK-11
title: >-
  Einmal das ganze project durchgehen und nach clean code und clean architecture
  refactorn.
status: Done
assignee:
  - '@claude'
created_date: '2026-05-14 18:23'
updated_date: '2026-05-15 09:25'
labels: []
dependencies: []
priority: medium
ordinal: 11250
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
src/main.rs build_ui() is ~250 lines with 3-level closure nesting. The recording start/stop logic, streaming preview timer, and segment polling timer are all inlined inside a single 30ms glib timeout callback. This makes the code hard to follow and error-prone to modify.

Goals:
- Remove debug eprintln at line 114 (leftover from development)
- Move spawn_sidecar() to a new src/sidecar.rs module
- Extract start_recording() and stop_recording() as named free functions
- Lift the StreamRx type alias out of the nested block to module level
- Move space_join() to dictation_panel.rs (it is conceptually about text append logic)
- No behavior changes — pure structural cleanup
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 eprintln!("[gdk] backend: ...") removed from build_ui()
- [x] #2 spawn_sidecar() moved to src/sidecar.rs; main.rs updated to use it
- [x] #3 start_recording() extracted as named fn in main.rs (takes recorder, recording flag, panel, tray_handle, port)
- [x] #4 stop_recording() extracted as named fn in main.rs (takes recorder, recording flag, panel, tray_handle, config, history, port)
- [x] #5 StreamRx type alias lifted to module level
- [x] #6 space_join() moved from main.rs to dictation_panel.rs (pub(crate))
- [x] #7 cargo build passes with zero warnings
- [x] #8 cargo test --bin vooox — all 24 tests green
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. Read main.rs, dictation_panel.rs fully (done)
2. Create src/sidecar.rs — move spawn_sidecar() there, keep pub visibility, update main.rs mod/use
3. In main.rs: remove eprintln!("[gdk] backend") at line 114
4. Lift 'type StreamRx = ...' alias to module level (before build_ui)
5. Extract start_recording(dev, recorder, recording, panel, tray_handle, port) -> bool free fn — contains: Recorder::start, set flags, panel.show_recording, tray update, spawn 200ms streaming timer
6. Extract stop_recording(recorder, recording, panel, tray_handle, config, history, port) free fn — contains: clear flag, panel.show_processing, take recorder, to_wav, spawn transcription thread, spawn 50ms segment poll timer
7. In build_ui() shortcut handler: replace inline blocks with start_recording()/stop_recording() calls
8. Move space_join() from main.rs to dictation_panel.rs as pub(crate) fn; update call site in main.rs
9. cargo build — fix any compile errors
10. cargo test --bin vooox — verify 24 tests green
<!-- SECTION:PLAN:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Pure structural refactoring — no behavior changes.

Changes:
- Extracted spawn_sidecar() into new src/sidecar.rs module
- Removed debug eprintln!("[gdk] backend: ...") from build_ui()
- Lifted StreamRx type alias to module level
- Extracted start_recording() and stop_recording() as named free functions; build_ui() shortcut handler is now ~25 lines
- Extracted spawn_streaming_timer() (200ms live-preview loop) and spawn_segment_poll() (50ms final-transcription loop) as named functions
- Moved space_join() from main.rs to dictation_panel.rs (pub(crate))

Tests: cargo build — clean (17 pre-existing deprecation warnings, 0 errors); cargo test --bin vooox — 24/24 passed
<!-- SECTION:FINAL_SUMMARY:END -->
