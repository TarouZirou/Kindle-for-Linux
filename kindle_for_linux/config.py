"""Persistent configuration and data paths (XDG compliant)."""

from __future__ import annotations

import json
import locale
import os
from dataclasses import asdict, dataclass, field
from pathlib import Path

APP_NAME = "kindle-for-linux"

# Kindle Cloud Reader endpoints per Amazon marketplace.
REGIONS: dict[str, tuple[str, str]] = {
    "jp": ("日本 (amazon.co.jp)", "https://read.amazon.co.jp/"),
    "us": ("United States (amazon.com)", "https://read.amazon.com/"),
    "uk": ("United Kingdom (amazon.co.uk)", "https://read.amazon.co.uk/"),
    "de": ("Deutschland (amazon.de)", "https://read.amazon.de/"),
    "fr": ("France (amazon.fr)", "https://read.amazon.fr/"),
    "es": ("España (amazon.es)", "https://read.amazon.es/"),
    "it": ("Italia (amazon.it)", "https://read.amazon.it/"),
    "ca": ("Canada (amazon.ca)", "https://read.amazon.ca/"),
    "au": ("Australia (amazon.com.au)", "https://read.amazon.com.au/"),
    "in": ("India (amazon.in)", "https://read.amazon.in/"),
    "br": ("Brasil (amazon.com.br)", "https://read.amazon.com.br/"),
    "mx": ("México (amazon.com.mx)", "https://read.amazon.com.mx/"),
}


def _xdg(var: str, fallback: str) -> Path:
    value = os.environ.get(var)
    base = Path(value) if value else Path.home() / fallback
    return base / APP_NAME


CONFIG_DIR = _xdg("XDG_CONFIG_HOME", ".config")
DATA_DIR = _xdg("XDG_DATA_HOME", ".local/share")
CACHE_DIR = _xdg("XDG_CACHE_HOME", ".cache")
CONFIG_FILE = CONFIG_DIR / "config.json"


def default_region() -> str:
    lang = os.environ.get("LANG") or (locale.getlocale()[0] or "")
    return "jp" if lang.lower().startswith("ja") else "us"


@dataclass
class Config:
    region: str = field(default_factory=default_region)
    width: int = 1100
    height: int = 800
    maximized: bool = False
    zoom: float = 1.0
    # Empty string = use WebKit's default (Safari compatible) user agent.
    user_agent: str = ""
    # Restore the last opened page (e.g. the book you were reading).
    restore_last_page: bool = True
    last_url: str = ""

    @property
    def home_url(self) -> str:
        return REGIONS.get(self.region, REGIONS["us"])[1]

    @classmethod
    def load(cls) -> "Config":
        try:
            raw = json.loads(CONFIG_FILE.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            return cls()
        known = {k: v for k, v in raw.items() if k in cls.__dataclass_fields__}
        cfg = cls(**known)
        if cfg.region not in REGIONS:
            cfg.region = default_region()
        cfg.zoom = min(max(float(cfg.zoom), 0.3), 3.0)
        return cfg

    def save(self) -> None:
        CONFIG_DIR.mkdir(parents=True, exist_ok=True)
        tmp = CONFIG_FILE.with_suffix(".tmp")
        tmp.write_text(json.dumps(asdict(self), indent=2, ensure_ascii=False), encoding="utf-8")
        tmp.replace(CONFIG_FILE)
