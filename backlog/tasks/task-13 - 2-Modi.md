---
id: TASK-13
title: 2 Modi
status: Done
assignee:
  - '@daniel'
created_date: '2026-05-18 09:17'
updated_date: '2026-05-18 11:29'
labels: []
dependencies: []
priority: high
ordinal: 4812.5
---

## Description

<!-- SECTION:DESCRIPTION:BEGIN -->
Es gibt zwei Modi. Modus 1, das Diktierfenster wird angezeigt mit der Transkription und Modus 2. Es wird nur ein Icon angezeigt, was zeigt, dass eine Transkription gerade aufgenommen wird und verarbeitet wird. Dieses Icon soll auch im Vordergrund bleiben, genauso wie das Diktierfenster beim Diktieren.
<!-- SECTION:DESCRIPTION:END -->

## Acceptance Criteria
<!-- AC:BEGIN -->
- [x] #1 Config besitzt Feld panel_mode (Window|Icon), default Window, in ~/.config/vooox/config.toml persistiert
- [x] #2 Kebab-Menü des Diktierfensters enthält Sektion 'Modus' mit zwei Radio-Items (Diktierfenster / Nur Icon), die den aktiven Modus markieren und beim Klick sofort umschalten
- [x] #3 Tray-Menü enthält 'Modus'-Submenü mit denselben zwei Optionen als CheckmarkItems, ebenfalls live umschaltbar
- [x] #4 Tray-Menüpunkt 'Diktierfenster' ruft das Aufnahmefenster im aktuellen Modus auf (im Icon-Modus erscheint das Pill, im Window-Modus das volle Panel)
- [x] #5 Im Icon-Modus zeigt das Fenster ein kompaktes, abgerundetes Pill (~80×40 px) mit: pulsierendem roten Punkt während Aufnahme, Live-Waveform aus audio::LevelMeter, kleinem mm:ss-Timer-Text
- [x] #6 Pill bleibt always-on-top wie das volle Panel, an gleicher Standardposition (Center-bottom des aktiven Monitors)
- [x] #7 Processing-State im Icon-Modus zeigt eine ruhige Animation (z. B. rotierende Punkte oder Spinner) statt der Waveform
- [x] #8 Done-State zeigt kurz grünes Häkchen (~1 s), dann blendet das Pill aus; Text liegt im Clipboard; History-Eintrag wird geschrieben
- [x] #9 Umschalten zwischen Modi während laufender Aufnahme ist verhindert oder sauber (kein Crash, kein Statusverlust)
- [x] #10 cargo build und cargo test --bin vooox laufen grün
<!-- AC:END -->

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. Config: enum PanelMode { Window, Icon } + Serde + Default=Window in src/config.rs; Field panel_mode hinzufügen.
2. Tray (src/tray.rs): TrayCommand::SetPanelMode(PanelMode) ergänzen; im Menu Submenu 'Modus' mit zwei CheckmarkItems; VoooxTray hält current mode für Checkmark-Anzeige; set_panel_mode(handle, mode) Helper analog set_recording.
3. dictation_panel.rs: zwei Layout-Modi im selben Window — Window-Mode wie heute; Icon-Mode = neues Pill via DrawingArea (Cairo) für Waveform + Label für Timer + roter Punkt via CSS @keyframes. apply_mode(&self, mode) schaltet Sichtbarkeit der Kinder + Größe + CSS-Klasse um.
4. Kebab-Menü: neue Sektion 'Modus' mit Stateful Action panel.mode (Variant String 'window'|'icon') — analog zur bestehenden model-Action.
5. main.rs: TrayCommand::SetPanelMode-Handler → Config.panel_mode = neue; cfg.save(); panel.apply_mode(...); Tray-Checkmark via set_panel_mode aktualisieren. Beim Start: panel.apply_mode(config.panel_mode).
6. Recording-Flow: show_recording startet zusätzlich Waveform-Animation (30 fps glib::timeout_add_local liest LevelMeter.last_level). show_processing schaltet auf rotierende Punkte. finish() zeigt grünes Häkchen und versteckt Pill nach 1 s (nur Icon-Mode); Window-Mode wie bisher offen lassen.
7. Cargo build + cargo test --bin vooox; manuell beide Modi durchklicken.
<!-- SECTION:PLAN:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Zwei Panel-Modi (Window / Icon) implementiert.

Änderungen:
- config.rs: Neues Feld panel_mode: PanelMode { Window, Icon }, default Window, TOML-persistiert. PanelMode::as_str / from_str für Serialisierung in GIO-Actions.
- tray.rs: Neues SubMenu 'Modus' mit zwei CheckmarkItems (live aus VoooxTray.panel_mode markiert). TrayCommand::SetPanelMode(PanelMode) ergänzt. spawn_tray nimmt initial_mode; neue Helper set_panel_mode für Live-Updates.
- dictation_panel.rs: Zweites Layout (pill_layout) im selben Window. Im Icon-Modus: kompaktes 150×40-Pill mit pulsierendem roten Punkt (CSS @keyframes), Live-Waveform aus 14 cairo-Bars getrieben vom bestehenden audio::LevelMeter, mm:ss-Timer. Processing-State zeigt gtk4::Spinner und gelben Dot. Done-State zeigt 1.1 s ein grünes ✓, danach blendet das Pill aus. Window-Modus unverändert. apply_mode(mode) schaltet Sichtbarkeit + default_size. Neue stateful Action panel.mode im Kebab-Menü als Sektion 'Modus'. Drag-Gesture auch auf dem Pill, damit es ebenfalls verschiebbar ist.
- main.rs: TrayCommand::SetPanelMode-Handler — speichert Config, ruft panel.apply_mode + tray::set_panel_mode (hält Tray-Checkmark, Kebab-State und Window-Layout synchron).

Im Icon-Modus wird der Text wie gewünscht nur ins Clipboard kopiert und ein History-Eintrag geschrieben; die Transkription wird nicht angezeigt.

Tests:
- cargo build (clean, nur pre-existing warnings).
- cargo test --bin vooox: 24/24 grün.
<!-- SECTION:FINAL_SUMMARY:END -->
