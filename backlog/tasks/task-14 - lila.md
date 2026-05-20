---
id: TASK-14
title: >-
  PTT hat einen lila kreis, aber die Balken sind noch rot. Bitte auch lila
  machen.
status: Done
assignee:
  - '@claude'
created_date: '2026-05-20 12:07'
updated_date: '2026-05-20 12:29'
labels: []
dependencies: []
priority: medium
ordinal: 11125
---

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. Pegelbalken werden in der Pill via DrawingArea + Cairo gezeichnet (build_waveform_area).
2. Aktuelle Farbe ist rot (vermutlich hardcoded). Phase liegt schon vor: PillPhase::Recording vs Processing.
3. Brauche zusätzlich Info ob PTT aktiv → entweder neue PillPhase-Variante PttRecording, oder shared bool/Cell.
4. Einfachste Lösung: Rc<Cell<bool>> ptt_active im Panel, in build_waveform_area mitnehmen, in der Draw-Callback Farbe wählen.
5. set_ptt_active(bool) im Panel auch dieses Cell setzen + queue_draw aufrufen.
<!-- SECTION:PLAN:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
PTT-Pegelbalken sind jetzt auch lila.

Änderungen:
- PillPhase um Variante RecordingPtt erweitert.
- build_waveform_area: zusätzliche Farbe (#c93cff = rgba 0.788, 0.235, 1.0) für RecordingPtt.
- set_ptt_active wechselt jetzt zusätzlich die Phase und ruft queue_draw auf der DrawingArea auf — die Balken färben sich sofort um, wenn die PTT-Schwelle überschritten wird, und springen beim Release zurück auf rot.

Tests: cargo build clean, 26/26 unit tests grün.
<!-- SECTION:FINAL_SUMMARY:END -->
