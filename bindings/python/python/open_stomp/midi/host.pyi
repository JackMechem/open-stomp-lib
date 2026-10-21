"""Host -> pedal commands: live CC/PC on channel 2 and SysEx writes."""

from typing import Optional

from ..catalog import Module
from ..records import Preset, Settings

LOOP_REC_PLAY_DUB: int
LOOP_STOP: int
LOOP_CLEAR: int
LOOP_UNDO_REDO: int

def cc(cc: int, val: int) -> bytes: ...
def program(index: int) -> bytes:
    """Select preset ``index`` (0-based, P01 = 0)."""
def head(module: Module, model: int, on: bool) -> Optional[bytes]:
    """Model + on/off of an effect module; None for LOOP/DRUM."""
def param(module: Module, slot: int, value: int) -> Optional[bytes]:
    """A module parameter by slot; None if the module has no such slot."""
def volume(value: int) -> bytes: ...
def page(module: Module) -> bytes:
    """Make the pedal display jump to a module's page."""
def looper_button(button: int) -> bytes: ...
def preset_name(source_slot: int, name: str) -> bytes: ...
def preset_write(preset: Preset) -> bytes: ...
def save_preset(source_slot: int, preset: Preset) -> list[bytes]:
    """The official app's save sequence: name, record, select the saved slot."""
def settings_write(settings: Settings) -> bytes: ...
