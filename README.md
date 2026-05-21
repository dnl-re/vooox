# vooox

Lokale Speech-to-Text-Desktop-App für Linux. Globalen Shortcut drücken, sprechen, nochmal drücken — der transkribierte Text wird direkt an der Cursor-Position eingefügt. Alles läuft offline auf dem eigenen Rechner.

## Features

- **Push-to-Talk oder Toggle** über globalen Shortcut
- **faster-whisper** (CTranslate2) als Engine — CPU oder optional CUDA-GPU
- **Streaming**: Segmente erscheinen im Overlay sobald Whisper sie liefert
- **Auto-Paste** an die zuletzt aktive Fenster-/Cursorposition mit Clipboard-Restore
- **Zwei-stufige Text-Injection**: `xdotool` → `enigo` (XTest-Fallback)
- **First-Run-Setup-Wizard**: System-Check, isoliertes Python-venv, optionale GPU-Pakete, Modell-Auswahl
- **Settings-UI** mit Live-Mikrofon-Pegel, Modell-Download/Löschen, GPU-Status, Shortcut-Bindung
- **Tray-Icon** und Verlauf der letzten 50 Transkriptionen (JSONL)
- **AppImage**-Distribution (~3 MB Binary, Python-Welt wird User-lokal aufgebaut)

## Architektur

```
Rust-Binary (vooox)
├── GTK4 UI-Thread     — Settings, Overlay-Pille, Tray
├── Audio-Thread       — cpal-Mikrofon-Capture
├── Shortcut-Thread    — rdev globaler Key-Listener
├── TextInjector       — xdotool → enigo
└── WhisperClient      — WebSocket zum Python-Sidecar

Python-Sidecar (whisper_server/server.py)
└── WebSocket-Server   — faster-whisper, streamt Segmente
```

Config: `~/.config/vooox/config.toml`
Verlauf: `~/.local/share/vooox/history.jsonl`
venv: `~/.local/share/vooox/venv/`
Modelle: `~/.cache/huggingface/hub/`

## Installation

### AppImage (empfohlen)

```bash
chmod +x vooox-x86_64.AppImage
./vooox-x86_64.AppImage
```

Beim ersten Start führt der Wizard durch System-Check, venv-Setup (~600 MB), optionale GPU-Installation und Modell-Auswahl.

> **Hinweis:** Nur unter X11 entwickelt und getestet. Wayland wird **nicht** unterstützt — globale Shortcuts und Text-Injection an Fremdfenster funktionieren dort nicht.

### Aus Source bauen

```bash
sudo apt install libgtk-4-dev libayatana-appindicator3-dev pkg-config
cargo build --release
./target/release/vooox
```

## Modelle

| Modell    | Größe   | Empfohlen für |
|-----------|---------|---------------|
| `tiny`    | 75 MB   | CPU, sehr schnell, geringere Qualität |
| `base`    | 145 MB  | CPU |
| `small`   | 480 MB  | CPU (Default) |
| `medium`  | 1.5 GB  | GPU |
| `large-v3`| 3 GB    | GPU, beste Qualität |

## GPU-Beschleunigung (optional)

Erkennt automatisch NVIDIA-GPUs mit Treiber ≥ 525. Installiert dann `nvidia-cublas-cu12` und `nvidia-cudnn-cu12` in das vooox-eigene venv — kein systemweiter CUDA-Toolkit-Install nötig. Umschaltbar über Settings → Whisper.

## Tests

```bash
cargo test --bin vooox                                            # Rust-Unittests
python3 -m pytest whisper_server/tests/test_server.py -q          # Sidecar-Tests
./target/debug/vooox --test-pipeline tests/fixtures/hello_de.wav  # Headless-E2E
```

## AppImage bauen

```bash
./packaging/build_appimage.sh
# → dist/vooox-x86_64.AppImage
```

## Backlog

Tasks und offene Themen werden mit [backlog.md](https://github.com/MrLesk/Backlog.md) verwaltet:

```bash
backlog task list --plain
backlog board
```

## Lizenz

MIT — siehe [LICENSE](LICENSE).
