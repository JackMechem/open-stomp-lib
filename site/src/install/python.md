# Use it from Python

The `open_stomp` Python package is the Rust library compiled in with PyO3: the same protocol, state tracking,
connection engine and NAM / IR conversion. Python ≥ 3.9.

## Install

**From a release wheel** (no Rust needed). Download the wheel for your platform from the
[releases page](https://github.com/JackMechem/open-stomp-lib/releases) and install it:

```sh
pip install open_stomp-0.4.0-*.whl
```

**From source** (Rust ≥ 1.88; on Linux also the ALSA headers, `libasound2-dev` / `alsa-lib-devel`):

```sh
git clone https://github.com/JackMechem/open-stomp-lib
pip install ./open-stomp-lib/bindings/python
```

Check it:

```sh
python -c "import open_stomp; print(open_stomp.__version__)"
```

## First script

```python
from open_stomp import Device, DeviceState, Module

state = DeviceState()
with Device() as dev:                      # finds and connects to the pedal by itself
    dev.select_preset(4)                   # P05
    dev.set_head(Module.AMP, 8, True)      # AMP on, model 8 (Mar J800)
    for u in dev:                          # blocks; Ctrl-C to stop
        if ev := state.feed(u):
            print(ev)                      # AMP      Mar J800 · GAIN = 64
```

Import a NAM model and a cab IR into user slots (slots are 0–9; this overwrites what's there):

```python
dev.import_nam_file(9, "Ampeg SVT.nam")    # optimised conversion, about a minute
dev.import_ir_file(9, "Ampeg 8x10.wav")    # optimised conversion, under a second
```

Or convert without a pedal and look at the result:

```python
from open_stomp import nam, ir

model = nam.Model.load("Ampeg SVT.nam")
c = nam.convert(model, measure="optimised")   # c.data: the 2048 floats the pedal gets
out = model.process(samples)                  # run the model on your own audio (mono, 48 kHz)
cab = ir.convert("Ampeg 8x10.wav", method="optimised")
```

## Examples

A checkout has two ready-made scripts:

```sh
python bindings/python/examples/monitor.py            # follow the pedal (--listen, --replay, --all)
python bindings/python/examples/shell.py              # an interactive shell over the whole API
```

## Next

The [Python guide](../lib/11-python-guide.md) covers the whole API: the engine, state, controlling and saving,
presets and settings, the catalogue, your own transport, capture replay and threading.
