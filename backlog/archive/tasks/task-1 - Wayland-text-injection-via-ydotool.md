---
id: TASK-1
title: Wayland text injection via ydotool
status: To Do
assignee: []
created_date: '2026-05-14 16:45'
updated_date: '2026-05-14 17:58'
labels: []
dependencies: []
priority: low
ordinal: 12000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Text injection into Wayland-native windows (GNOME Terminal, Firefox Wayland mode) currently silently fails. enigo/XTest events are dropped by the Wayland compositor. ydotool uses kernel uinput and works universally but requires the ydotoold user service.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 ydotool detected and used when available
- [ ] #2 Clear error message with install instructions when ydotool is missing and target is Wayland-native
- [ ] #3 systemctl --user enable --now ydotool documented in CLAUDE.md
<!-- AC:END -->
