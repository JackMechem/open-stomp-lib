# 11 — Python guide

`open_stomp` for Python wraps the Rust library ([10](10-library-guide.md)) with PyO3. Everything is the same code: the
protocol, `DeviceState`, the request queue and the connection engine. This guide shows the Python side; the Rust guide
has the background on each piece.

* [Install](#install)
* [Modules](#modules)
* [Quick start](#quick-start)
* [The device engine](#the-device-engine)
* [Following state](#following-state)
* [Controlling the pedal](#controlling-the-pedal)
* [Presets and settings](#presets-and-settings)
* [The catalogue](#the-catalogue)
* [Bring your own transport](#bring-your-own-transport)
* [Capture logs and replay](#capture-logs-and-replay)
* [Conventions](#conventions)
* [Threads and Ctrl-C](#threads-and-ctrl-c)
* [Testing and development](#testing-and-development)

## Install

```sh
pip install ./bindings/python        # from a checkout of this repo (builds with maturin)
```

Needs Rust ≥ 1.88 and, on Linux, the ALSA headers (`libasound2-dev` / `alsa-lib-devel`). Release wheels are
attached to the [GitHub releases](https://github.com/JackMechem/open-stomp-lib/releases). Python ≥ 3.9.

## Modules

The package mirrors the Rust crate. The most used types are also importable from `open_stomp` directly.

| Module | Contents | Rust |
|---|---|---|
| `open_stomp` | `Device`, `Update`, `DeviceState`, `Event`, `Origin`, `Module`, `ParamDef`, `Preset`, `ModuleState`, `Settings`, `Msg`, `Assembler`, `is_stomp_port`, `USB_ID`, `__version__` | crate root |
| `open_stomp.pack` | `pack`, `unpack` | `pack` |
| `open_stomp.midi` | `Msg`, `Assembler`, `hex`, `frame`, `MFR`, `UID`, `PEDAL_CH`, `HOST_CH` | `midi` |
| `open_stomp.midi.req` | `version()`, `product()`, `settings()`, `cur_state()`, `drum()`, `looper()`, `tuner()`, `edit_buffer()`, `ir_slot(i)`, `preset(i)`, `user_amp(i)` | `midi::req` |
| `open_stomp.midi.host` | `cc`, `program`, `head`, `param`, `volume`, `page`, `looper_button`, `preset_name`, `preset_write`, `save_preset`, `settings_write`, `LOOP_*` | `midi::host` |
| `open_stomp.catalog` | `Module`, `ParamDef`, `drum_pattern_name`, `NOTES`, `DRUM_STYLES`, `*_MODELS`, `MICS`, `POSITIONS` | `catalog` |
| `open_stomp.records` | `Preset`, `ModuleState`, `Settings`, `preset_label`, `PRESET_COUNT`, `PRESET_LEN`, `DEFAULT_CHAIN` | `records` |
| `open_stomp.state` | `DeviceState`, `Event`, `Origin`, `Looper`, `Drum`, `Tuner`, `looper_state_name` | `state` |
| `open_stomp.queue` | `RequestQueue`, `Request`, `Done`, `handshake`, `follow_up` | `queue` |
| `open_stomp.capture` | `parse`, `Entry` | `capture` |
| `open_stomp.upload` | `Chunk`, `Reassembler`, `UserAmpInfo`, `user_amp_upload`, `user_amp_delete`, `user_ir_upload`, `user_ir_name`, `data_frames`, `check`, `floats_to_bytes`, `bytes_to_floats` | `upload` |
| `open_stomp.device` | `Device`, `Update` | `device` |

## Quick start

```python
from open_stomp import Device, DeviceState

state = DeviceState()
with Device() as dev:                  # finds and connects to the pedal by itself
    for u in dev:                      # blocks; Ctrl-C to stop
        ev = state.feed(u)
        if ev and ev.category != "protocol" and not ev.redundant:
            print(ev)                  # AMP      Mar J800 · GAIN = 64
```

`bindings/python/examples/monitor.py` is the full version (`--listen`, `--port`, `--replay`, `--all`).
`bindings/python/examples/shell.py` is an interactive shell over the whole API: inspect presets and state, send
live edits, back up, and save presets/settings (with confirmation). `-c CMD` runs commands non-interactively.

## The device engine

`Device(...)` starts the engine thread immediately; it scans the MIDI ports, connects to the pedal, runs the startup
sync and reports everything as `Update`s.

```python
Device(*, client_name="Open Stomp", auto_connect=True, sync=True, reply_timeout=1.0, scan_interval=1.5)
```

| Argument | Meaning |
|---|---|
| `client_name` | MIDI client name shown by the OS |
| `auto_connect` | Connect to the first pedal-looking port, and again after it is replugged |
| `sync` | May send anything at all (startup reads and edits). `False` = listen only |
| `reply_timeout` | Seconds a SysEx request waits for its reply before the queue moves on |
| `scan_interval` | Seconds between port rescans (hotplug) |

Receiving:

| Call | |
|---|---|
| `for u in dev:` | Block for each update; ends when the device is closed |
| `dev.recv(timeout=None)` | Next update, or `None` after `timeout` seconds |
| `dev.try_recv()` | Next update if one is waiting, else `None` |
| `dev.drain()` | All waiting updates as a list (for a UI loop) |

Controlling the connection: `connect(port)`, `disconnect()`, `resync()`, `set_sync(on)`, `replay(path, speed=4.0)`,
`stop_replay()`. Stop the engine with `close()` or by leaving the `with` block; after that every call raises
`RuntimeError`.

`Update.kind` says which fields are set:

| `kind` | Fields |
|---|---|
| `"ports"` | `ports`: MIDI input port names (at start and on every hotplug) |
| `"status"` | `status` = `"disconnected"` / `"connected"` (`port`, `output`) / `"replaying"` (`file`, `progress`) |
| `"sync"` | `done`, `total`: startup sync progress; `0, 0` = none running |
| `"message"` | `at`, `origin`, `bytes`: one complete MIDI message, `Origin.PEDAL` or `Origin.HOST` |
| `"mark"` | `at`, `text`: a `# MARK` note in a replayed capture |
| `"info"` | `at`, `text`, `error`: connected, unplugged, no reply, refused send… |
| `"reset"` | A new session (connect, resync, replay): reset your `DeviceState` |

`at` is Unix time in seconds like `time.time()` (the original capture time during replay).

## Following state

`DeviceState` is the pedal model. `state.feed(update)` applies `"message"` updates and returns their `Event`, clears
the model on `"reset"`, and returns `None` for anything else. `state.apply(data, origin)` applies raw bytes directly.

```python
ev = state.apply(b"\xb0\x12\x40", Origin.PEDAL)
ev.category, ev.module, ev.subject, ev.value    # ('effect', Module.AMP, 'Black Twin · GAIN', '64')
```

| `Event` | |
|---|---|
| `category` | `"preset"`, `"effect"`, `"volume"`, `"navigation"`, `"looper"`, `"drum"`, `"tuner"`, `"settings"`, `"device"`, `"protocol"`, `"unknown"` |
| `module` | The `Module` for effect events |
| `label`, `subject`, `value` | Display text: `"AMP"`, `"Mar J800 · GAIN"`, `"64"`; `str(ev)` joins them |
| `coalesce` | Same key on consecutive events = one gesture (knob sweep) |
| `redundant` | Changed nothing |
| `origin`, `raw` | Direction and bytes |

`DeviceState` attributes: `firmware`, `model_id`, `product`, `presets` (99 × `Preset | None`), `current`, `live`,
`live_known`, `page`, `looper`, `drum`, `tuner`, `settings`, `user_amps`, `user_irs`; methods `modified()`,
`stored(i)`, `preset_name(i)`, `model_name(module, model)`, `select_preset(i)`.

> **Attributes are snapshots.** `state.live`, `state.presets[3]`, `preset.modules` and so on return copies.
> Changing a copy never changes the model; the model changes only through `apply`/`feed`. To edit, take a copy, change
> it, and send it (see [saving](#saving-a-preset)).

## Controlling the pedal

Live changes (heard at once, stored only when you save):

```python
from open_stomp import Module
from open_stomp.midi import host

dev.select_preset(4)                         # P05
dev.set_head(Module.AMP, 8, True)            # Mar J800, on (model and on/off share one CC)
dev.set_param(Module.AMP, 0, 100)            # GAIN 100
dev.set_param(Module.EQ, 3, 60)              # EQ band 4 +10
dev.set_volume(80)
dev.show_page(Module.DELAY)                  # pedal display jumps to DELAY
dev.looper_button(host.LOOP_REC_PLAY_DUB)
dev.send(host.cc(0x3F, 1))                   # any CC from docs/02 (drum on)
```

To switch a block off without changing its model, pass the current one:
`dev.set_head(m, state.live.module(m).model, False)`.

Sends are refused (an `"info"` update with `error=True`) in listen-only mode or without an output port. The engine
reports each message it sends as an `Origin.HOST` `"message"` update, so `state.feed` keeps the model in step with
your own edits.

### Saving a preset

```python
p = state.live                     # a copy of the edit buffer with all live edits
source = state.current             # the slot the edits were made on
p.index = 21                       # store to P22 (or keep `source` to overwrite)
p.name = "Lead"                    # <= 16 printable ASCII; others become '?'
dev.save_preset(source, p)         # name, record, then select the saved slot
```

### Uploading a user AMP or IR

```python
dev.import_user_amp(2, "Jcm800", floats)      # 2048 floats -> USER AMP 03 (factor=1.0 by default)
dev.import_user_ir(0, "My Cab", floats)       # USER IR 01
dev.delete_user_amp(2)
```

Converting `.nam`/`.wav` files into the 2048 floats isn't implemented yet ([05](05-uploads-nam-ir.md)); a captured
conversion can be recovered with `upload.Chunk.parse` + `upload.Reassembler` and uploaded to any slot. A bad slot or
data raises `ValueError`.

### Changing system settings

```python
s = state.settings                 # start from the pedal's record: unknown bytes survive
s.input_trim = 13                  # 0-20 = -10..+10 dB
s.loopback = False
dev.write_settings(s)
```

> Preset and settings writes match the official app byte for byte but have not yet been run on hardware from this
> library. Back up your presets first (keep the `Preset`s from `state.presets`).

## Presets and settings

```python
from open_stomp import Module, Preset

p = Preset.from_packed(payload)    # a 0x0B/0x0C SysEx body; ValueError if it isn't one
p.name, p.index, p.volume, p.chain # chain: list of 7 Modules (GATE is always first, not listed)
p.module(Module.AMP)               # ModuleState(on=True, model=8, params=[...12 values])
p.set_module(Module.AMP, on=False, model=3, params=[50, 50, 50])
p.set_param(Module.AMP, 0, 100)
p.to_packed(), p.to_raw()          # bytes; unknown bytes of the original record are kept
p.same_sound(other)                # ignores unused parameter slots
```

`Preset()` is an empty preset. `copy.copy`/`deepcopy` and `p.copy()` work. Assigning a whole list to `p.modules`
also works. `Settings` has `input_trim` (+ `input_trim_db`), `eq_bass`, `eq_mid`, `eq_treble`, `wireless`,
`loopback`, `raw`, `from_packed`, `to_packed`, `to_raw`.

## The catalogue

```python
from open_stomp import Module

Module.AMP.models[8]               # 'Mar J800'
[p.name for p in Module.AMP.params(8)]   # ['GAIN', 'BASS', 'MIDDLE', 'TREBLE', 'PRESENCE', 'MASTER']
mic = Module.CAB.param(3, 0)
mic.kind, mic.choices[:2], mic.format(4) # ('enum', ['E906', 'M160'], 'SM57')
Module.from_param_cc(0x12)         # (Module.AMP, 0)
Module.AMP.head_cc, Module.AMP.param_ccs, Module.AMP.label   # 1, (18, 6), 'AMP'
```

`Module` is an int-like enum (`int(Module.EQ) == 7`, hashable); `Module.ALL` and `Module.EFFECTS` list them.
`ParamDef.kind` is `"knob"` (0–100), `"bipolar"` (0–100, 50 = centre) or `"enum"`; `format(v)` gives the value as
the official app shows it. `catalog.drum_pattern_name(13)` is `"Rock 04"`; `catalog.NOTES` names tuner notes.

## Bring your own transport

The protocol pieces work on plain bytes, so you can drive the pedal through any MIDI library (`mido`,
`python-rtmidi`, Web MIDI via Pyodide, BLE-MIDI…):

```python
from open_stomp import Assembler, DeviceState, Msg, Origin
from open_stomp.queue import RequestQueue, follow_up, handshake

q, asm, state = RequestQueue(timeout=1.0), Assembler(), DeviceState()
q.start_sync(handshake())

def pump():                                   # call regularly
    while (r := q.pop()) is not None:         # nothing while a request is in flight
        midi_out.send_message(list(r.bytes))
        state.apply(r.bytes, Origin.HOST)
        q.sent(r)
    q.expire()                                # move on from unanswered requests

def on_midi_in(data: bytes):                  # any chunking
    for msg in asm.push(data):
        q.on_message(msg)                     # releases the next request if it answers
        for r in follow_up(msg):
            q.read(r)
        state.apply(msg, Origin.PEDAL)
```

`q.progress` is `(done, total)` of the sync. For tests, `sent(r, at=…)` and `expire(at=…)` take a time in seconds
since the queue was created, so timeouts can be driven without sleeping.

## Capture logs and replay

```python
from open_stomp import DeviceState, capture

state = DeviceState()
for e in capture.parse(open("captures/logs/01-main-session.log").read()):
    if e.kind == "mark":
        print("=====", e.text)
    else:
        print(state.apply(e.bytes, e.origin))
```

`dev.replay(path, speed)` plays a log through the engine instead, with its timing, as if a pedal were connected.

## Conventions

* **Bytes in:** every argument that takes MIDI data accepts `bytes`, `bytearray` or a list of ints.
  **Bytes out:** always `bytes`.
* **Indices are 0-based:** preset 0 is P01 (`records.preset_label(0) == "P01"`), model 8 is shown as `#09`.
* **Errors:** bad records raise `ValueError`, out-of-range slots `IndexError`, using a closed `Device`
  `RuntimeError`. Builders that cannot apply return `None` (`host.head(Module.LOOP, …)`).
* **Snapshots:** objects returned from attributes are copies (see [above](#following-state)).

## Threads and Ctrl-C

The engine runs on its own thread. `recv`, iteration, `close` and the end of a `with` block release the GIL, so
other Python threads keep running, and they wake at least every 100 ms to let Ctrl-C raise `KeyboardInterrupt`. A
`Device` can be used from several threads (one receiving, others sending commands). The bindings also support
free-threaded Python.

## Testing and development

```sh
cd bindings/python
pip install maturin pytest mypy
maturin develop                                            # build into the active venv
pytest                                                     # tests driven by captures/logs
python -m mypy.stubtest open_stomp --ignore-missing-stub   # the .pyi stubs match the module
```

| Test | Checks |
|---|---|
| `test_protocol.py` | Packing, parsing, reassembly, command builders (captured bytes), catalogue, record encode/decode, copy semantics |
| `test_captures.py` | The whole captured session applies cleanly; every record re-encodes byte for byte; the captured *save as* is rebuilt; the startup sync through `RequestQueue` against a fake pedal answering from the capture |
| `test_device.py` | The engine end to end through replay, timeouts, close semantics |

Layout: `bindings/python/src/*.rs` (one file per Rust module), `python/open_stomp/` (`__init__.py` and the `.pyi`
stubs), `tests/`, `examples/`. The Python package version is the workspace version in the root `Cargo.toml`.
