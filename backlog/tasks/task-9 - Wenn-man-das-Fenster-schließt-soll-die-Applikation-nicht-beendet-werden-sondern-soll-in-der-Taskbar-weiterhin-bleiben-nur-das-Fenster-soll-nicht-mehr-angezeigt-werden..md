---
id: TASK-9
title: >-
  Wenn man das Transkriptions-Fenster schließt, soll die Applikation nicht
  beendet werden, sondern soll in der Taskbar weiterhin bleiben, nur das Fenster
  soll nicht mehr angezeigt werden.
status: Done
assignee: []
created_date: '2026-05-14 17:57'
updated_date: '2026-05-14 18:14'
labels: []
dependencies: []
priority: high
ordinal: 9000
---

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Added connect_close_request handler that calls win.hide() and returns Propagation::Stop — window hides, app stays alive in tray.
<!-- SECTION:FINAL_SUMMARY:END -->
