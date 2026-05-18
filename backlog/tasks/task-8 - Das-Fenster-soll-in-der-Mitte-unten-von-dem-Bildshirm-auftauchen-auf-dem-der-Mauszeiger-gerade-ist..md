---
id: TASK-8
title: >-
  Das Transkriptions-Fenster soll in der Mitte unten von dem Bildshirm
  auftauchen, auf dem der Mauszeiger gerade ist.
status: Done
assignee: []
created_date: '2026-05-14 17:54'
updated_date: '2026-05-14 19:01'
labels: []
dependencies: []
priority: high
ordinal: 8000
---

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Window now positions itself center-bottom on the monitor containing the mouse cursor when recording starts. Uses xdotool getmouselocation for cursor position, GDK4 monitor API for geometry (scaled to X11 physical pixels), and xdotool windowmove via X11Surface XID. Falls back to default_size() when window.width()/height() return 0 on first show.
<!-- SECTION:FINAL_SUMMARY:END -->
