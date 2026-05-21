---
id: TASK-4
title: Wayland global shortcut capture
status: To Do
assignee: []
created_date: '2026-05-14 16:45'
updated_date: '2026-05-14 17:58'
labels: []
dependencies: []
priority: low
ordinal: 14000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
rdev currently captures global shortcuts via X11/XWayland (DISPLAY env var). On a pure Wayland session without XWayland this would fail silently. Proper Wayland global shortcuts require the xdg-desktop-portal or GNOME-specific D-Bus API (org.gnome.Shell.Grab).
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 Shortcut works in a pure Wayland session (no DISPLAY set)
- [ ] #2 Fallback to rdev/X11 when XWayland is available
- [ ] #3 Clear error shown when neither X11 nor Wayland shortcut capture is available
<!-- AC:END -->
