# open-stomp (Python)

Python bindings for [open-stomp](https://github.com/JackMechem/open-stomp-lib), the library for the **SORAN Dual
Stomp Lite** guitar pedal over USB-MIDI. The Rust library is compiled in (PyO3), so you get the same tested protocol
code, state tracking and connection engine, with no Python dependencies.

```python
from open_stomp import Device, DeviceState, Module

state = DeviceState()
with Device() as dev:                 # finds and connects to the pedal by itself
    for u in dev:                     # blocks; Ctrl-C to stop
        ev = state.feed(u)            # applies messages, resets on reconnect
        if ev and ev.category != "protocol" and not ev.redundant:
            print(ev)                 # AMP      Mar J800 · GAIN = 64
        if u.kind == "sync" and u.total == 0 and state.current is not None:
            dev.set_param(Module.AMP, 0, 100)   # turn the active preset's gain up
```

* Import NAM amp models: `dev.import_nam_file(slot, "amp.nam")`, or `open_stomp.nam` to run and convert models.
* Python ≥ 3.9 (one `abi3` wheel per platform), Linux / macOS / Windows.
* Fully typed (`py.typed` stubs, checked against the compiled module with `stubtest`).
* Blocking calls release the GIL, and Ctrl-C interrupts them.

## Install

From a checkout (needs Rust ≥ 1.88; on Linux also `libasound2-dev libdbus-1-dev` / `alsa-lib-devel dbus-devel`):

```sh
pip install ./bindings/python            # from the repository root
```

Or download a wheel from the repository's [Releases](https://github.com/JackMechem/open-stomp-lib/releases) and
`pip install` it.

## Documentation

The **[Python guide](https://github.com/JackMechem/open-stomp-lib/blob/main/docs/11-python-guide.md)** covers the
engine, state tracking, editing, saving presets and settings, the catalogue, your own MIDI transport, capture replay
and testing. The [protocol docs](https://github.com/JackMechem/open-stomp-lib/tree/main/docs) describe the pedal
itself.

## Development

```sh
cd bindings/python
python -m venv .venv && . .venv/bin/activate
pip install maturin pytest mypy
maturin develop                          # build and install in place (re-run after Rust changes)
pytest                                   # tests driven by ../../captures/logs
python -m mypy.stubtest open_stomp --ignore-missing-stub   # stubs match the compiled module
python examples/monitor.py --replay ../../captures/logs/01-main-session.log --speed 8
python examples/shell.py                 # interactive shell: inspect, edit, save, back up (`help` lists commands)
```

License: MIT OR Apache-2.0.
