---
id: TASK-7
title: >-
  Das Transkriptions-Fensters soll nicht den fokus bekommen (d.h. der bestehende
  fokus wird nicht gelöst), aber trotzdem im Vordergrund sein.
status: Done
assignee: []
created_date: '2026-05-14 17:54'
updated_date: '2026-05-14 18:19'
labels: []
dependencies: []
priority: high
ordinal: 4000
---

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Changed show_recording() to call window.show() instead of window.present(). present() is an explicit focus request in GTK4; show() maps the window without claiming focus, letting GNOME's focus-stealing prevention keep focus on the active app.
<!-- SECTION:FINAL_SUMMARY:END -->
