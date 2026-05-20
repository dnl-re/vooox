---
id: TASK-16
title: 'Einstellungen aktivieren die Mikros, deaktivieren sie aber nicht mehr.'
status: Done
assignee:
  - '@claude'
created_date: '2026-05-20 12:08'
updated_date: '2026-05-20 12:37'
labels: []
dependencies: []
priority: high
ordinal: 5437.5
---

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. build_microphone_tab leakt aktuell LevelMeter per std::mem::forget → bei jedem Settings-Öffnen werden ALLE Mikros aktiviert und nie wieder geschlossen.
2. Pegel-Meter per Mikro nur on-demand starten: ToggleButton 'Pegel testen' pro Zeile.
3. Pro Zeile eigenes Rc<RefCell<Option<LevelMeter>>>. Toggle on → start + spawnen Update-Timer. Toggle off → None setzen, Timer terminiert.
4. Sammeln aller Meter-Cells in einer tab-scoped Liste; im connect_close_request der ApplicationWindow alle auf None setzen, damit auch beim Schließen via X-Button alle Streams stoppen.
5. ListBox-Reihen-Layout: [Radio] [Mic-Name] [Toggle 'Pegel'] [LevelBar].
<!-- SECTION:PLAN:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Mikrofon-Pegel werden in den Einstellungen nicht mehr automatisch aktiviert.

Problem:
- build_microphone_tab hat für jedes gelistete Eingabegerät sofort einen audio::LevelMeter gestartet und ihn per std::mem::forget geleakt. Bei jedem Öffnen der Einstellungen wurden also ALLE Mikros aktiviert und liefen danach für immer weiter.

Lösung:
- Pro Mikro-Zeile eine ToggleButton 'Pegel testen'. Aus = kein Stream. An = LevelMeter::start + glib::timeout_add_local für die LevelBar; beim Ausschalten wird das Rc<RefCell<Option<LevelMeter>>> auf None gesetzt, der Stream droppt und der Timer beendet sich beim nächsten Tick.
- Tab-scoped Vec hält Referenzen auf alle Meter-Cells. ApplicationWindow::connect_close_request iteriert beim Schließen darüber und nullt jede Zelle, sodass auch beim X-Button-Close alle Streams sauber gestoppt werden.
- Layout: [Radio] [Name] [Toggle 'Pegel testen'] [LevelBar].

Tests: cargo build clean, 26/26 unit tests grün.
<!-- SECTION:FINAL_SUMMARY:END -->
