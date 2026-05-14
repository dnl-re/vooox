---
id: TASK-3
title: Wayland overlay positioning via gtk4-layer-shell
status: To Do
assignee: []
created_date: '2026-05-14 16:45'
labels: []
dependencies: []
priority: low
ordinal: 3000
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
The overlay window positioning (bottom-right corner etc.) currently relies on GTK4 window hints that only work reliably on X11. On Wayland, proper overlay positioning requires gtk4-layer-shell (wlr-layer-shell protocol). Without it, the overlay may appear in the wrong position or steal focus.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [ ] #1 gtk4-layer-shell crate integrated
- [ ] #2 Overlay appears at configured corner without stealing focus on Wayland
- [ ] #3 Graceful fallback for compositors without layer-shell support
<!-- AC:END -->
