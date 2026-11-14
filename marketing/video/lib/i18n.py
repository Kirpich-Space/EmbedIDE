"""Tiny localisation helper: compose scripts call tr(en, ru); VIDEO_LANG=ru selects the Russian dub."""
import os

LANG = os.environ.get("VIDEO_LANG", "en")


def set_lang(lang):
    global LANG
    LANG = lang


def tr(en, ru=None):
    return ru if (LANG == "ru" and ru is not None) else en


CHIPS = {
    "REAL RECORDING · embed 0.1.0-alpha.1": "РЕАЛЬНАЯ ЗАПИСЬ · embed 0.1.0-alpha.1",
    "ANIMATION · not application footage": "АНИМАЦИЯ · не запись программы",
    "TIME-LAPSE ×{x}": "УСКОРЕНО ×{x}",
}


def chip(key, **kw):
    text = CHIPS[key] if LANG == "ru" else key
    return text.format(**kw)
