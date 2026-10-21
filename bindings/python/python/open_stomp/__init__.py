"""Talk to the SORAN Dual Stomp guitar pedal over USB-MIDI.

Python bindings for the `open-stomp` Rust library. The modules mirror the crate:

* ``open_stomp.pack``      8-in-7 bitstream ``pack`` / ``unpack``
* ``open_stomp.midi``      ``Msg``, ``Assembler``, ``hex``, ``frame``; ``midi.req`` reads, ``midi.host`` commands
* ``open_stomp.catalog``   ``Module``, ``ParamDef``, model lists, drum patterns, note names
* ``open_stomp.records``   ``Preset``, ``ModuleState``, ``Settings``
* ``open_stomp.state``     ``DeviceState``, ``Event``, ``Origin``
* ``open_stomp.queue``     ``RequestQueue``, ``handshake``, ``follow_up``
* ``open_stomp.capture``   capture log reader
* ``open_stomp.device``    ``Device``: the background connection engine

Guide: https://github.com/JackMechem/open-stomp-lib/blob/main/docs/11-python-guide.md
"""

import sys as _sys

from . import _native
from ._native import USB_ID, __version__, is_stomp_port

# Expose the extension's submodules as real modules, so `import open_stomp.midi` and
# `from open_stomp.midi.host import cc` work like any package.
for _name in ("pack", "midi", "catalog", "records", "state", "queue", "capture", "device"):
    _mod = getattr(_native, _name)
    _mod.__name__ = f"{__name__}.{_name}"
    _sys.modules[_mod.__name__] = _mod
    globals()[_name] = _mod
for _name in ("req", "host"):
    _mod = getattr(_native.midi, _name)
    _mod.__name__ = f"{__name__}.midi.{_name}"
    _sys.modules[_mod.__name__] = _mod
del _name, _mod

from .catalog import Module, ParamDef  # noqa: E402
from .device import Device, Update  # noqa: E402
from .midi import Assembler, Msg  # noqa: E402
from .records import ModuleState, Preset, Settings  # noqa: E402
from .state import DeviceState, Event, Origin  # noqa: E402

__all__ = [
    "USB_ID",
    "__version__",
    "is_stomp_port",
    "pack",
    "midi",
    "catalog",
    "records",
    "state",
    "queue",
    "capture",
    "device",
    "Assembler",
    "Device",
    "DeviceState",
    "Event",
    "Module",
    "ModuleState",
    "Msg",
    "Origin",
    "ParamDef",
    "Preset",
    "Settings",
    "Update",
]
