---
id: TASK-6
title: Rust integration tests in tests/integration/
status: To Do
assignee: []
created_date: '2026-05-14 16:45'
updated_date: '2026-05-14 18:23'
labels: []
dependencies: []
priority: low
ordinal: 11750
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
tests/integration/ directory exists but contains no test files. Planned tests: sidecar_test.rs (start real Python sidecar, send WAV, assert transcription contains expected words) and pipeline_test.rs (audio→WAV→sidecar→MockInjector, assert final injected text).
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 sidecar_test.rs: starts Python sidecar, sends hello_de.wav, asserts result contains 'hallo'
- [ ] #2 pipeline_test.rs: full pipeline with MockInjector, asserts text is non-empty
- [ ] #3 Tests run with cargo test (not --bin, but integration)
- [ ] #4 CI-friendly: skip gracefully if sidecar deps not available
<!-- AC:END -->
