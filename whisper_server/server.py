#!/usr/bin/env python3
"""
vooox whisper sidecar — WebSocket server wrapping faster-whisper.

Protocol (JSON messages over WebSocket):
  C→S {"type":"health"}                      → S→C {"type":"ok","model":"small","language":"de"}
  C→S {"type":"models"}                      → S→C {"type":"models","list":["tiny","small",...]}
  C→S {"type":"config","model":"X","language":"Y"} → S→C {"type":"config_ok"}
  C→S {"type":"transcribe","audio_b64":"..."}
      → S→C {"type":"segment","text":"...","start":0.0,"end":1.2}  (one or more)
      → S→C {"type":"done","full_text":"...","language":"de","duration_ms":420}
  C→S {"type":"shutdown"}                    → server exits cleanly
"""

import asyncio
import base64
import io
import json
import os
import socket
import sys
import tempfile
import wave
from pathlib import Path

import websockets
from faster_whisper import WhisperModel

# ── config ──────────────────────────────────────────────────────────────────

def _cuda_available() -> bool:
    # CTranslate2 is the inference backend of faster-whisper and ships with
    # its own CUDA runtime detection — no torch dependency needed.
    try:
        import ctranslate2
        return ctranslate2.get_cuda_device_count() > 0
    except (ImportError, AttributeError):
        return False


DEFAULT_MODEL = os.environ.get("VOOOX_MODEL", "small")
DEFAULT_LANGUAGE = os.environ.get("VOOOX_LANGUAGE", "de")
# VOOOX_FORCE_CPU=1 lets the user opt out of GPU even when CUDA libs are
# present (useful on laptops where battery > speed).
FORCE_CPU = os.environ.get("VOOOX_FORCE_CPU", "").strip() not in ("", "0", "false", "False")
DEVICE = "cuda" if (_cuda_available() and not FORCE_CPU) else "cpu"


def _list_local_models() -> list[str]:
    """Return model names that are already downloaded in the HF cache."""
    cache_root = Path.home() / ".cache" / "huggingface" / "hub"
    names = []
    if cache_root.exists():
        for p in cache_root.iterdir():
            # typical: models--Systran--faster-whisper-small
            if p.is_dir() and "faster-whisper" in p.name:
                name = p.name.split("faster-whisper-")[-1]
                names.append(name)
    # always include a baseline so the list is never empty
    if not names:
        names = [DEFAULT_MODEL]
    return sorted(set(names))


# ── state ────────────────────────────────────────────────────────────────────

class State:
    def __init__(self):
        self.model_name: str = DEFAULT_MODEL
        self.language: str = DEFAULT_LANGUAGE
        self._model: WhisperModel | None = None

    def _load_model(self, name: str, allow_download: bool = False) -> WhisperModel:
        compute = "float16" if DEVICE == "cuda" else "int8"
        # local_files_only=True ensures we never trigger an implicit HF download
        # from a transcribe call. Explicit downloads go through ensure_model().
        return WhisperModel(
            name,
            device=DEVICE,
            compute_type=compute,
            local_files_only=not allow_download,
        )

    def get_model(self) -> WhisperModel:
        if self._model is None:
            self._model = self._load_model(self.model_name, allow_download=False)
        return self._model

    def ensure_model(self, name: str) -> None:
        """Force-load (and download if missing) the named model, then cache it."""
        self._model = self._load_model(name, allow_download=True)
        self.model_name = name

    def set_config(self, model: str | None, language: str | None):
        changed = False
        if model and model != self.model_name:
            self.model_name = model
            self._model = None  # force reload on next use
            changed = True
        if language:
            self.language = language
        return changed


state = State()


# ── handlers ─────────────────────────────────────────────────────────────────

async def handle_health(ws):
    await ws.send(json.dumps({
        "type": "ok",
        "model": state.model_name,
        "language": state.language,
        "device": DEVICE,
    }))


async def handle_models(ws):
    await ws.send(json.dumps({
        "type": "models",
        "list": _list_local_models(),
    }))


async def handle_config(ws, msg: dict):
    state.set_config(msg.get("model"), msg.get("language"))
    await ws.send(json.dumps({"type": "config_ok"}))


async def handle_ensure_model(ws, msg: dict):
    name = msg.get("model") or state.model_name
    try:
        # Run the blocking download in a thread so the WS event loop stays
        # responsive (so the client can still send a cancel/shutdown).
        await asyncio.to_thread(state.ensure_model, name)
        await ws.send(json.dumps({"type": "ready", "model": name}))
    except Exception as e:  # noqa: BLE001 — surface any failure to the UI
        await ws.send(json.dumps({"type": "error", "msg": f"ensure_model: {e}"}))


async def handle_transcribe(ws, msg: dict):
    import time
    audio_b64 = msg.get("audio_b64", "")
    audio_bytes = base64.b64decode(audio_b64)

    # write to a temp WAV file — faster-whisper works best with files
    with tempfile.NamedTemporaryFile(suffix=".wav", delete=False) as f:
        f.write(audio_bytes)
        tmp_path = f.name

    try:
        t0 = time.monotonic()
        try:
            model = state.get_model()
        except Exception as e:  # noqa: BLE001
            await ws.send(json.dumps({
                "type": "error",
                "msg": f"model '{state.model_name}' not available locally — "
                       f"open Settings → Whisper to download it ({e})",
            }))
            return
        lang = state.language if state.language != "auto" else None
        segments, info = model.transcribe(
            tmp_path,
            language=lang,
            beam_size=5,
            vad_filter=True,
            vad_parameters={"min_silence_duration_ms": 300},
        )
        full_parts = []
        for seg in segments:
            text = seg.text.strip()
            if not text:
                continue
            full_parts.append(text)
            await ws.send(json.dumps({
                "type": "segment",
                "text": text,
                "start": round(seg.start, 3),
                "end": round(seg.end, 3),
            }))
        elapsed_ms = int((time.monotonic() - t0) * 1000)
        await ws.send(json.dumps({
            "type": "done",
            "full_text": " ".join(full_parts),
            "language": info.language,
            "duration_ms": elapsed_ms,
        }))
    finally:
        os.unlink(tmp_path)


# ── main loop ────────────────────────────────────────────────────────────────

_shutdown_event = asyncio.Event()


async def handler(ws):
    try:
        async for raw in ws:
            try:
                msg = json.loads(raw)
            except json.JSONDecodeError:
                await ws.send(json.dumps({"type": "error", "msg": "invalid JSON"}))
                continue

            t = msg.get("type")
            if t == "health":
                await handle_health(ws)
            elif t == "models":
                await handle_models(ws)
            elif t == "config":
                await handle_config(ws, msg)
            elif t == "ensure_model":
                await handle_ensure_model(ws, msg)
            elif t == "transcribe":
                await handle_transcribe(ws, msg)
            elif t == "shutdown":
                await ws.send(json.dumps({"type": "bye"}))
                _shutdown_event.set()
                return
            else:
                await ws.send(json.dumps({"type": "error", "msg": f"unknown type: {t}"}))
    except Exception:
        pass  # client disconnected without close frame — ignore


async def main():
    # pick a free port
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        port = s.getsockname()[1]

    # Note: no model pre-load. Sidecar must start fast even without any model
    # downloaded yet — explicit downloads are triggered by ensure_model from
    # the setup wizard / settings.
    async with websockets.serve(handler, "127.0.0.1", port, max_size=100 * 1024 * 1024):
        # signal readiness to the Rust parent — must be the first stdout line
        print(f"VOOOX_PORT={port}", flush=True)
        await _shutdown_event.wait()


if __name__ == "__main__":
    asyncio.run(main())
