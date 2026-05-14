#!/usr/bin/env python3
"""Generate test WAV fixtures using pyttsx3/espeak or a sine fallback."""
import os
import struct
import wave
import math

OUT = os.path.join(os.path.dirname(__file__), "fixtures")
os.makedirs(OUT, exist_ok=True)

RATE = 16000

def write_wav(path, samples):
    with wave.open(path, "w") as wf:
        wf.setnchannels(1)
        wf.setsampwidth(2)
        wf.setframerate(RATE)
        raw = struct.pack(f"<{len(samples)}h", *samples)
        wf.writeframes(raw)

def sine(freq, duration):
    n = int(RATE * duration)
    return [int(math.sin(2 * math.pi * freq * i / RATE) * 16000) for i in range(n)]

def silence(duration):
    return [0] * int(RATE * duration)

# Try TTS first, fall back to beep
for lang, text, fname in [
    ("de", "Hallo, das ist ein Test.", "hello_de.wav"),
    ("en", "Hello, this is a test.", "hello_en.wav"),
]:
    path = os.path.join(OUT, fname)
    generated = False
    try:
        import subprocess
        result = subprocess.run(
            ["espeak-ng", "-v", lang, "-w", path, text],
            capture_output=True, timeout=10
        )
        if result.returncode == 0 and os.path.exists(path):
            print(f"Generated {fname} via espeak")
            generated = True
    except Exception:
        pass

    if not generated:
        # Fallback: 3 tones that whisper will transcribe to something
        # (not real speech, but tests that the pipeline doesn't crash)
        samples = sine(440, 0.3) + silence(0.1) + sine(550, 0.3) + silence(0.1) + sine(660, 0.3)
        write_wav(path, samples)
        print(f"Generated {fname} via sine fallback (espeak not available)")

print("Fixtures ready.")
