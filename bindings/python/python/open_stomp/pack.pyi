"""8-in-7 packing: 8-bit structs carried as a little-endian 7-bit bitstream (docs/03)."""

from ._types import BytesLike

def pack(data: BytesLike) -> bytes:
    """8-bit struct bytes -> 7-bit MIDI bytes (LSB first)."""

def unpack(data: BytesLike) -> bytes:
    """7-bit MIDI bytes -> 8-bit struct bytes. A trailing partial byte is dropped."""
