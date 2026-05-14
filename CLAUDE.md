# vooox

Local speech-to-text desktop app for Linux. Press a global shortcut to start recording, press again to stop — transcribed text is typed at the cursor.

## Architecture

```
Rust binary (vooox)
├── GTK4 UI thread     — settings window, overlay indicator, tray
├── Audio thread       — cpal microphone capture
├── Shortcut thread    — rdev global key listener
├── TextInjector       — ydotool → xdotool → enigo fallback chain
└── WhisperClient      — WebSocket client to the Python sidecar

Python sidecar (whisper_server/server.py)
└── WebSocket server   — faster-whisper, streams segments as they arrive
```

The sidecar is started as a subprocess; it prints `VOOOX_PORT=XXXX` on stdout when ready.

Config: `~/.config/vooox/config.toml`  
History: `~/.local/share/vooox/history.jsonl`

## Build & run

```bash
cargo build
./target/debug/vooox
```

## Test commands

```bash
cargo test --bin vooox                                     # 24 Rust unit tests
python3 -m pytest whisper_server/tests/test_server.py -q  # 7 Python sidecar tests
./target/debug/vooox --test-pipeline tests/fixtures/hello_de.wav  # headless E2E
```

## Crate API gotchas

| Crate | Version | Gotcha |
|---|---|---|
| `gtk4` | 0.11 | Needs `libgtk-4-dev`; `set_accept_focus` is GTK3 — use `set_focusable(false)` |
| `cpal` | 0.17 | `SampleRate` is `pub type SampleRate = u32` — no `.0` field |
| `enigo` | 0.6 | Use `use enigo::Keyboard;` (not `enigo::Text`), call `.text(text)` |
| `ksni` | 0.3 | Use `features = ["blocking","tokio"]`; call `tray.spawn()` via `TrayMethods` trait |
| `glib` | 0.22 | Needs explicit `glib = "0.22"` dependency even though gtk4 re-exports it |
| `tokio-tungstenite` | 0.29 | `WebSocketConfig` is `#[non_exhaustive]` — use `Default::default()` then set fields |

## Text injection (Wayland)

The app tries backends in order: `ydotool` → `xdotool` → `enigo` (XTest).  
For Wayland-native windows (GNOME Terminal, Firefox in Wayland mode), `ydotool` is required:

```bash
sudo apt install ydotool
systemctl --user enable --now ydotool
```

## Audio device

On PipeWire/GNOME systems select **"PulseAudio / PipeWire"** in Settings → Mikrofon.  
This follows whatever input device is set in GNOME Sound Settings.

## Regenerate test fixtures

```bash
python3 tests/gen_fixtures.py   # requires espeak-ng
```
