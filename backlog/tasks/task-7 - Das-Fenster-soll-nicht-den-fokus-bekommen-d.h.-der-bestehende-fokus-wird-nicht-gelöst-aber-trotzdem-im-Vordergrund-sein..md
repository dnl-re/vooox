---
id: TASK-7
title: >-
  Das Transkriptions-Fensters soll nicht den fokus bekommen (d.h. der bestehende
  fokus wird nicht gelöst), aber trotzdem im Vordergrund sein.
status: Done
assignee: []
created_date: '2026-05-14 17:54'
updated_date: '2026-05-14 18:41'
labels: []
dependencies: []
priority: high
ordinal: 4000
---

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Added gdk4-x11 dependency. Before present(), realize() the window and set _NET_WM_USER_TIME=0 via X11Surface::set_user_time(0). This signals to the X11 WM that the window was not opened by a recent user action so it raises without granting keyboard focus.
<!-- SECTION:FINAL_SUMMARY:END -->
