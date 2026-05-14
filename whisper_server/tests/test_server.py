"""pytest suite for the vooox whisper WebSocket sidecar."""
import asyncio
import base64
import json
import os
import signal
import subprocess
import sys
import time
from pathlib import Path

import pytest
import websockets

FIXTURES = Path(__file__).parent.parent.parent / "tests" / "fixtures"
SERVER_PY = Path(__file__).parent.parent / "server.py"


# ── server fixture ────────────────────────────────────────────────────────

@pytest.fixture(scope="session")
def sidecar_port():
    """Start the sidecar once for the whole test session, return its port."""
    proc = subprocess.Popen(
        [sys.executable, str(SERVER_PY)],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    line = proc.stdout.readline().strip()
    assert line.startswith("VOOOX_PORT="), f"unexpected sidecar output: {line!r}"
    port = int(line.split("=")[1])

    # wait for the server to be ready
    deadline = time.monotonic() + 30
    ws_url = f"ws://127.0.0.1:{port}"
    while time.monotonic() < deadline:
        try:
            asyncio.get_event_loop().run_until_complete(_health_check(ws_url))
            break
        except Exception:
            time.sleep(0.3)
    else:
        proc.kill()
        raise RuntimeError("sidecar did not start in time")

    yield port

    proc.kill()
    proc.wait(timeout=5)


async def _health_check(url):
    async with websockets.connect(url) as ws:
        await ws.send(json.dumps({"type": "health"}))
        resp = json.loads(await ws.recv())
        assert resp["type"] == "ok"


def ws_url(port):
    return f"ws://127.0.0.1:{port}"


async def send_recv(port, msg):
    async with websockets.connect(ws_url(port)) as ws:
        await ws.send(json.dumps(msg))
        return json.loads(await ws.recv())


# ── tests ─────────────────────────────────────────────────────────────────

def test_health(sidecar_port):
    resp = asyncio.get_event_loop().run_until_complete(
        send_recv(sidecar_port, {"type": "health"})
    )
    assert resp["type"] == "ok"
    assert "model" in resp
    assert "device" in resp


def test_models(sidecar_port):
    resp = asyncio.get_event_loop().run_until_complete(
        send_recv(sidecar_port, {"type": "models"})
    )
    assert resp["type"] == "models"
    assert isinstance(resp["list"], list)
    assert len(resp["list"]) >= 1


def test_config_change(sidecar_port):
    resp = asyncio.get_event_loop().run_until_complete(
        send_recv(sidecar_port, {"type": "config", "model": "tiny", "language": "en"})
    )
    assert resp["type"] == "config_ok"
    # verify health reflects new config
    health = asyncio.get_event_loop().run_until_complete(
        send_recv(sidecar_port, {"type": "health"})
    )
    assert health["language"] == "en"


def test_transcribe_returns_done(sidecar_port):
    """Transcribing a real WAV file must yield a 'done' message."""
    wav_path = FIXTURES / "hello_en.wav"
    if not wav_path.exists():
        pytest.skip("hello_en.wav fixture not found")

    audio_b64 = base64.b64encode(wav_path.read_bytes()).decode()
    results = asyncio.get_event_loop().run_until_complete(
        _collect_transcription(sidecar_port, audio_b64)
    )
    msg_types = [r["type"] for r in results]
    assert "done" in msg_types, f"no 'done' received: {results}"


async def _collect_transcription(port, audio_b64):
    messages = []
    async with websockets.connect(ws_url(port)) as ws:
        await ws.send(json.dumps({"type": "transcribe", "audio_b64": audio_b64}))
        while True:
            msg = json.loads(await ws.recv())
            messages.append(msg)
            if msg["type"] in ("done", "error"):
                break
    return messages


def test_transcribe_streaming(sidecar_port):
    """For longer audio, segments should arrive before or with 'done'."""
    wav_path = FIXTURES / "hello_de.wav"
    if not wav_path.exists():
        pytest.skip("hello_de.wav fixture not found")

    audio_b64 = base64.b64encode(wav_path.read_bytes()).decode()
    results = asyncio.get_event_loop().run_until_complete(
        _collect_transcription(sidecar_port, audio_b64)
    )
    # at minimum we get a 'done'
    assert any(r["type"] == "done" for r in results)
    done = next(r for r in results if r["type"] == "done")
    assert "full_text" in done
    assert "duration_ms" in done


def test_unknown_type_returns_error(sidecar_port):
    resp = asyncio.get_event_loop().run_until_complete(
        send_recv(sidecar_port, {"type": "nonexistent_command"})
    )
    assert resp["type"] == "error"


def test_invalid_json_returns_error(sidecar_port):
    async def send_raw(port, raw):
        async with websockets.connect(ws_url(port)) as ws:
            await ws.send(raw)
            return json.loads(await ws.recv())

    resp = asyncio.get_event_loop().run_until_complete(
        send_raw(sidecar_port, "this is not json!!!")
    )
    assert resp["type"] == "error"
