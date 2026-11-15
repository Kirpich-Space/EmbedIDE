"""Minimal ElevenLabs client (standard library only). The API key is read from $ELEVENLABS_API_KEY or from
~/.config/embedide/elevenlabs.key (chmod 600) and is never printed or logged."""
import json, os, urllib.error, urllib.request
from pathlib import Path

API = "https://api.elevenlabs.io/v1"
MODEL = os.environ.get("ELEVENLABS_MODEL", "eleven_multilingual_v2")


def key():
    k = os.environ.get("ELEVENLABS_API_KEY", "").strip()
    if not k:
        f = Path.home() / ".config" / "embedide" / "elevenlabs.key"
        if f.exists():
            k = f.read_text().strip()
    if not k:
        raise SystemExit("no ElevenLabs key: set ELEVENLABS_API_KEY or write ~/.config/embedide/elevenlabs.key")
    return k


def _req(path, body=None, accept="application/json"):
    data = json.dumps(body).encode() if body is not None else None
    r = urllib.request.Request(API + path, data=data, headers={"xi-api-key": key(), "Accept": accept,
                                                                "Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(r, timeout=120) as resp:
            return resp.read()
    except urllib.error.HTTPError as e:
        raise SystemExit(f"ElevenLabs HTTP {e.code}: {e.read()[:300].decode(errors='replace')}")


def quota():
    """(used, limit) characters of the current period. Costs nothing."""
    s = json.loads(_req("/user/subscription"))
    return s["character_count"], s["character_limit"]


def voices():
    return json.loads(_req("/voices"))["voices"]


def speak(text, voice_id, out_mp3, stability=0.55, similarity=0.8, style=0.15, speed=1.0, previous_text=None,
          next_text=None):
    body = {"text": text, "model_id": MODEL,
            "voice_settings": {"stability": stability, "similarity_boost": similarity, "style": style,
                               "use_speaker_boost": True, "speed": speed}}
    if previous_text:
        body["previous_text"] = previous_text
    if next_text:
        body["next_text"] = next_text
    Path(out_mp3).write_bytes(_req(f"/text-to-speech/{voice_id}?output_format=mp3_44100_128", body, "audio/mpeg"))
