"""In-process splatter coverage engine (PyO3 + maturin)."""

from __future__ import annotations

from splatter._core import Session as _Session
from splatter._core import SPLAT_CACHE_SCHEMA_VERSION, input_sha256

__all__ = ["Session", "SPLAT_CACHE_SCHEMA_VERSION", "get_session", "input_sha256", "reset_session"]

_session: _Session | None = None


def get_session(*, mirror_root: str, verbose: bool = False, reset: bool = False) -> _Session:
    """Return a process-wide session with a resident DEM mosaic."""
    global _session
    if reset or _session is None or str(_session.mirror_root()) != mirror_root or _session.verbose() != verbose:
        _session = _Session(mirror_root, verbose=verbose)
    return _session


def reset_session() -> None:
    """Drop the cached session (tests / mirror path changes)."""
    global _session
    _session = None
