---
id: TASK-18
title: Migrate ComboBoxText to DropDown+StringList
status: To Do
assignee: []
created_date: '2026-05-21 08:38'
labels: []
dependencies: []
priority: low
ordinal: 12500
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Three ComboBoxText widgets (Whisper model dropdown, language dropdown in Settings, model dropdown in setup wizard) trigger 24 deprecation warnings on every build. gtk4 deprecated ComboBoxText in 4.10 in favor of DropDown + StringList. Also covers the single cpal DeviceTrait::name deprecation in src/audio.rs:62 — but note that switching cpal device identification from name() to id()/description() would break configs that store the old name string, so that part needs a migration shim.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Whisper model dropdown in src/settings.rs uses DropDown+StringList
- [ ] #2 Language dropdown in src/settings.rs uses DropDown+StringList
- [ ] #3 Model dropdown in src/setup_window.rs uses DropDown+StringList
- [ ] #4 cargo build emits no deprecation warnings related to ComboBoxText
- [ ] #5 Decision documented (in Final Summary) on whether to migrate cpal DeviceTrait::name and how to keep config backward compatibility
<!-- AC:END -->
