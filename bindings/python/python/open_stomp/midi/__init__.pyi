"""MIDI message parsing, SysEx framing and stream reassembly (docs/02, docs/03)."""

from typing import Literal, Optional, final

from .._types import BytesLike
from . import host as host
from . import req as req

MFR: bytes
UID: int
PEDAL_CH: int
HOST_CH: int

@final
class Msg:
    """One parsed MIDI message; ``kind`` says which fields are set."""

    @staticmethod
    def parse(data: BytesLike) -> Msg: ...
    @property
    def kind(self) -> Literal["cc", "pc", "nep", "notify", "short", "other"]: ...
    @property
    def ch(self) -> Optional[int]:
        """0-based channel (0 = pedal -> host, 1 = host -> pedal); cc and pc."""
    @property
    def cc(self) -> Optional[int]: ...
    @property
    def val(self) -> Optional[int]: ...
    @property
    def prog(self) -> Optional[int]: ...
    @property
    def cmd(self) -> Optional[int]:
        """SysEx command (nep)."""
    @property
    def dir(self) -> Optional[int]:
        """0 read, 1 write, 2 reply, 3 ack (nep)."""
    @property
    def what(self) -> Optional[int]:
        """Notified object (notify)."""
    @property
    def short_kind(self) -> Optional[int]:
        """Short frame kind: 0x00/0x10 version, 0x60 product (short)."""
    @property
    def payload(self) -> Optional[bytes]:
        """Payload (nep, notify, short) or the whole message (other)."""
    def __eq__(self, other: object, /) -> bool: ...

@final
class Assembler:
    """Reassembles a raw byte stream (possibly split SysEx) into whole messages."""

    def __new__(cls) -> Assembler: ...
    def push(self, data: BytesLike) -> list[bytes]: ...

def hex(data: BytesLike) -> str: ...
def frame(cmd: int, dir: int, payload: BytesLike) -> bytes:
    """Build ``F0 00 21 7A 70 <cmd> <dir> <payload> F7``."""
