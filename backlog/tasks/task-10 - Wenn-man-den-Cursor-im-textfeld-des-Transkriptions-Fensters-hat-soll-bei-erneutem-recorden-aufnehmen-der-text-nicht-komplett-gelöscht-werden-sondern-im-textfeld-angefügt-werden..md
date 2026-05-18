---
id: TASK-10
title: >-
  Wenn man den Cursor im textfeld des Transkriptions-Fensters hat, soll bei
  erneutem recorden/aufnehmen, der text nicht komplett gelöscht werden, sondern
  im textfeld angefügt werden.
status: Done
assignee: []
created_date: '2026-05-14 18:03'
updated_date: '2026-05-14 19:16'
labels: []
dependencies: []
priority: medium
ordinal: 11500
---

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Append mode: if cursor is in the text view when recording starts, new transcription appends to existing text instead of clearing it. Implemented via base_text field that stores pre-recording buffer content; set_transcript() always writes base_text + new_text (idempotent), preventing duplication between streaming interim and final segments. finish() reads the full buffer.
<!-- SECTION:FINAL_SUMMARY:END -->
