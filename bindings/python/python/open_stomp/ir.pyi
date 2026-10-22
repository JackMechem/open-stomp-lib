"""IR audio files for the pedal's user CAB slots: read, write, resample, convert (docs/05)."""

import os
from collections.abc import Sequence
from typing import Literal, Union, final

IR_TAPS: int
SAMPLE_RATE: int
PEDAL_TAPS: int

@final
class ConvertedIr:
    @property
    def data(self) -> list[float]:
        """The 2048 floats to upload."""
    @property
    def gain(self) -> float: ...
    @property
    def ir(self) -> list[float]: ...

def load_wav(path: Union[str, os.PathLike[str]]) -> tuple[int, list[list[float]]]: ...
def save_wav(
    path: Union[str, os.PathLike[str]], sample_rate: int, channels: Sequence[Sequence[float]], float: bool = False
) -> None: ...
def convert(path: Union[str, os.PathLike[str]], method: Literal["app", "optimised"] = "app") -> ConvertedIr: ...
def resample(samples: Sequence[float], src: int, dst: int, like_app: bool = False) -> list[float]: ...
