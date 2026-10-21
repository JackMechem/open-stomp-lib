"""Module / model / parameter catalogue (docs/02)."""

from typing import ClassVar, Literal, Optional, final

NOTES: list[str]
DRUM_STYLES: list[str]
OD_MODELS: list[str]
AMP_MODELS: list[str]
CAB_MODELS: list[str]
MOD_MODELS: list[str]
DELAY_MODELS: list[str]
REVERB_MODELS: list[str]
MICS: list[str]
POSITIONS: list[str]

@final
class Module:
    """A block in the signal chain."""

    OD_FX: ClassVar[Module]
    AMP: ClassVar[Module]
    CAB: ClassVar[Module]
    MOD: ClassVar[Module]
    DELAY: ClassVar[Module]
    REVERB: ClassVar[Module]
    GATE: ClassVar[Module]
    EQ: ClassVar[Module]
    LOOP: ClassVar[Module]
    DRUM: ClassVar[Module]
    ALL: ClassVar[list[Module]]
    EFFECTS: ClassVar[list[Module]]

    @staticmethod
    def from_index(i: int) -> Optional[Module]: ...
    @staticmethod
    def from_param_cc(cc: int) -> Optional[tuple[Module, int]]: ...
    @property
    def index(self) -> int: ...
    @property
    def label(self) -> str:
        """Display name, e.g. ``"OD/FX"``."""
    @property
    def is_effect(self) -> bool: ...
    @property
    def head_cc(self) -> Optional[int]: ...
    @property
    def param_ccs(self) -> Optional[tuple[int, int]]:
        """(first parameter CC, number of parameter CCs)."""
    @property
    def models(self) -> list[str]: ...
    def model_name(self, model: int) -> str: ...
    def params(self, model: int) -> list[ParamDef]: ...
    def param(self, model: int, slot: int) -> Optional[ParamDef]: ...
    def __int__(self) -> int: ...
    def __eq__(self, other: object, /) -> bool: ...
    def __hash__(self) -> int: ...

@final
class ParamDef:
    @property
    def slot(self) -> int: ...
    @property
    def name(self) -> str: ...
    @property
    def kind(self) -> Literal["knob", "bipolar", "enum"]: ...
    @property
    def choices(self) -> Optional[list[str]]: ...
    def format(self, value: int) -> str: ...

def drum_pattern_name(value: int) -> str: ...
