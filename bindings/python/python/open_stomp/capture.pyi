"""Capture log reader (docs/07)."""

from typing import Literal, Optional, final

from .state import Origin

@final
class Entry:
    @property
    def kind(self) -> Literal["message", "mark"]: ...
    @property
    def ms(self) -> int: ...
    @property
    def text(self) -> Optional[str]: ...
    @property
    def origin(self) -> Optional[Origin]: ...
    @property
    def bytes(self) -> Optional[bytes]: ...

def parse(text: str) -> list[Entry]: ...
