---
id: TASK-15
title: 'Das automatische Einfügen klappt generell, aber nicht in einer Shell'
status: Done
assignee:
  - '@claude'
created_date: '2026-05-20 12:07'
updated_date: '2026-05-20 12:13'
labels: []
dependencies: []
priority: high
ordinal: 2718.75
---

## Implementation Plan

<!-- SECTION:PLAN:BEGIN -->
1. Backlog-Task analysieren: in Shells (GNOME Terminal, konsole, alacritty etc.) ist Ctrl+V meist NICHT der Paste-Shortcut sondern Ctrl+Shift+V.
2. Ziel-Window-Klasse via xdotool getwindowclassname auslesen.
3. Wenn die Klasse zu einem bekannten Terminal passt → ctrl+shift+v statt ctrl+v senden.
4. Fallback auf ctrl+v wenn Klasse nicht ermittelbar.
5. Build + Tests + manuell verifizieren.
<!-- SECTION:PLAN:END -->

## Implementation Notes

<!-- SECTION:NOTES:BEGIN -->
Implementiert: window_class() in x11_window.rs holt WM_CLASS via xdotool getwindowclassname. paste_key_for(xid) in dictation_panel.rs wählt ctrl+shift+v wenn Klasse zu einem bekannten Terminal passt, sonst ctrl+v. Liste deckt GNOME Terminal, konsole, xterm, alacritty, kitty, foot, wezterm, terminator, tilix, ptyxis und weitere ab.
<!-- SECTION:NOTES:END -->

## Final Summary

<!-- SECTION:FINAL_SUMMARY:BEGIN -->
Auto-Paste funktioniert jetzt auch in Terminals.

Problem: Standard-Terminals interpretieren Ctrl+V nicht als Paste (SIGQUIT/Line-Kill/no-op). Vorher hat das schlicht nichts eingefügt oder den Shell-Prompt verschmutzt.

Lösung:
- Neuer x11_window::window_class(xid) Helper über xdotool getwindowclassname.
- paste_key_for(xid) in dictation_panel.rs entscheidet anhand der WM_CLASS, ob ctrl+v oder ctrl+shift+v gesendet wird.
- is_terminal_class deckt die gängigen Terminal-Klassen ab (GNOME Terminal, konsole, alacritty, kitty, foot, wezterm, terminator, tilix, ptyxis, urxvt, etc.) per case-insensitive substring match.
- Fallback bleibt ctrl+v, wenn die Klasse nicht ermittelbar ist.

Tests: cargo build clean, alle 23 unit tests grün.
<!-- SECTION:FINAL_SUMMARY:END -->
