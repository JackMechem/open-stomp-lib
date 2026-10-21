"""Typing helpers (not part of the public API)."""
from typing import Union
from collections.abc import Sequence

# bytes, bytearray or a sequence of ints 0-255
BytesLike = Union[bytes, bytearray, Sequence[int]]
