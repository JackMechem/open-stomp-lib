# open-stomp

A Rust library for the SORAN Dual Stomp Lite guitar pedal ·
Version **0.2.0** · Linux · macOS · Windows · [changelog](CHANGELOG.md)

`open-stomp` talks to the **SORAN Dual Stomp Lite** guitar multi-effects pedal (USB `3962:2000`, "JME STOMPBOX")
over USB-MIDI. It implements the protocol reverse engineered from the vendor's *Stomp Manager* app, and this repo
documents that protocol in full. [Open Stomp Manager](https://github.com/JackMechem/open-stomp-manager), the open
cross-platform editor, is built on it.

> Probably also applies to the Dual Stomp and Tiny Stomp (same firmware family, same app). Only the Dual Stomp Lite on
> firmware **V1.1.1** with Stomp Manager **1.0.2** has been tested.

## What it does

* **Parse and build every message**: live CC/PC control, vendor SysEx frames, the 8-in-7 packed records, request
  and command builders for everything the official app sends.
* **Track the pedal's state**: `DeviceState` applies traffic in either direction and keeps the 99 presets, the live
  edit buffer, the active preset, looper, drum, tuner, settings and user NAM/IR slot names. It also describes each
  message in plain English (`AMP · Mar J800 · GAIN = 64`).
* **Encode presets and settings** for writing, keeping unknown bytes intact. Tested byte for byte against the
  official app's captured writes.
* **Pace requests** the way the pedal needs: one SysEx in flight, reply matching, timeouts (`RequestQueue`, pure).
* **Import NAM amp models** (feature `nam`): load and run `.nam` files (a Rust port of NeuralAmpModelerCore) and
  convert them for the pedal with the official app's algorithm, reverse engineered (`Device::import_nam_file`).
* **Run the connection** (feature `device`): find the pedal, connect, follow hotplug, perform the official app's
  startup sync, send edits, report everything. It can also replay a capture log so you can develop without a pedal.

The protocol layer has no I/O and no dependencies. The device layer uses [`midir`](https://crates.io/crates/midir)
(ALSA, CoreMIDI, WinMM); the NAM layer uses `serde_json` and `rustfft`.

## Quick start

```toml
[dependencies]
open-stomp = { git = "https://github.com/JackMechem/open-stomp-lib", tag = "v0.2.0" }
```

```rust
use open_stomp::device::{Config, Device, Update};
use open_stomp::{DeviceState, Module};

let dev = Device::spawn(Config::default()); // finds and connects to the pedal by itself
let mut state = DeviceState::default();
while let Ok(update) = dev.recv() {
    match update {
        Update::Reset => state = DeviceState::default(),
        Update::Message { origin, bytes, .. } => {
            let ev = state.apply(&bytes, origin);
            println!("{:8} {} = {}", ev.category.label(), ev.subject, ev.value);
        }
        _ => {}
    }
}

// control it (the pedal's live sound; nothing is stored until you save):
dev.select_preset(4);                    // P05
dev.set_head(Module::Amp, 8, true);      // AMP on, model 8 (Mar J800)
dev.set_param(Module::Amp, 0, 100);      // GAIN 100
```

From **Python** (same library, compiled in with PyO3; see the [Python guide](docs/11-python-guide.md)):

```sh
pip install ./bindings/python
```

```python
from open_stomp import Device, DeviceState

state = DeviceState()
with Device() as dev:
    for u in dev:
        if ev := state.feed(u):
            print(ev)                    # AMP      Mar J800 · GAIN = 64
```

Without the pedal:

```sh
cargo run --example monitor -- --replay captures/logs/01-main-session.log --speed 8
cargo run --example decode -- captures/logs/01-main-session.log
```

With it: `cargo run --example monitor` (add `--listen` to send nothing), `cargo run --example ports`.

The [library guide](docs/10-library-guide.md) covers the engine, state tracking, saving presets, settings, the
catalogue, using your own MIDI transport, replay and testing. `cargo doc --open` has the API reference.

## The protocol in brief

* The pedal is a **class-compliant USB Audio 2.0 + USB-MIDI** device. No driver, no libusb, on any OS.
* **Live control is plain MIDI.** The pedal reports on **channel 1** and you command it on **channel 2** with the
  **same CC numbers**. Values are the 0–100 numbers the app shows. Each module has a head CC
  `(off ? 0x40 : 0) | model`.
  ```
  C1 04        -> select preset P05
  B1 01 40     -> amp OFF (model 0)        B1 01 08 -> amp ON, model 8 (Mar J800)
  B1 12 64     -> amp GAIN = 100
  B1 44 01     -> looper REC/PLAY/DUB
  ```
* **Bulk data** (presets, names, settings, looper/drum/tuner records, NAM/IR uploads) uses vendor SysEx
  `F0 00 21 7A 70 <cmd> <dir> … F7` (read/write/reply/ack), strictly request → response. Big records are 8-bit
  structs packed into a 7-bit little-endian bitstream.

Try it from a Linux shell:

```sh
amidi -l                                            # find "JME STOMPBOX", e.g. hw:2,0,0
amidi -p hw:2,0,0 -S 'F0 00 21 7A 00 F7' -d -t 1    # -> F0 00 21 7A 10 56 31 2E 31 2E 31 ... ("V1.1.1")
amidi -p hw:2,0,0 -d                                # turn knobs on the pedal and watch the CCs
```

## Status

| Area | Protocol | In the library |
|---|---|---|
| Device identification, handshake | ✅ decoded | ✅ `req::`, `queue::handshake`, engine sync |
| Every effect module / model / parameter ↔ CC (both directions) | ✅ all 8 modules | ✅ `catalog`, `host::head/param` |
| Preset select, delta CC dumps | ✅ | ✅ |
| Looper, drum, tuner | ✅ | ✅ state + `host::` commands |
| Preset record (110 B) | ✅ layout from 99 presets (some bytes unknown) | ✅ decode + encode, unknown bytes kept |
| Preset rename / save-as | ✅ sequence captured | ✅ `host::save_preset`, byte-exact vs capture |
| System settings | ✅ (some bytes unknown) | ✅ decode + encode |
| User AMP (NAM) / user IR upload transport | ✅ chunk protocol decoded | ✅ `upload`, byte-exact vs capture |
| NAM inference and NAM → device conversion | ✅ algorithm recovered ([docs/05](docs/05-uploads-nam-ir.md)) | ✅ `nam`, verified against the app |
| IR (audio) → device conversion | ❌ open | ❌ |

Live CCs are verified on hardware. Preset and settings **writes** match the official app byte for byte but have not
yet been run on hardware from this code. Back up your presets first.

## Documentation

This repository is the home of the protocol and reverse-engineering documentation. The app's own docs (user guide,
building, architecture) are in [open-stomp-manager](https://github.com/JackMechem/open-stomp-manager).

| | |
|---|---|
| [01 Overview](docs/01-overview.md) | Hardware, USB interfaces, protocol summary, app architecture |
| [02 MIDI CC reference](docs/02-midi-cc-reference.md) | **Every CC/PC**: all models and params, looper, drum, tuner |
| [03 SysEx protocol](docs/03-sysex-protocol.md) | Framing, command table, handshake, encodings, captured write sequences |
| [04 Data structures](docs/04-data-structures.md) | Preset, settings, IR/NAM slot, drum/loop/tuner records |
| [05 NAM & IR uploads](docs/05-uploads-nam-ir.md) | Upload transport and what's known about the conversion |
| [06 Rust/Linux guide](docs/06-rust-linux-guide.md) | Crates, architecture, protocol code in isolation, roadmap |
| [07 Capture methodology](docs/07-capture-methodology.md) | How it was sniffed on macOS and how to make new captures |
| [08 Static analysis](docs/08-static-analysis.md) | App binary (Flutter/Dart AOT), native libs, firmware notes |
| [09 Open questions](docs/09-open-questions.md) | What's left and how to find out |
| [10 Library guide](docs/10-library-guide.md) | **Using this crate**: engine, state, editing, saving, recipes, testing |
| [11 Python guide](docs/11-python-guide.md) | **Using it from Python**: the same API through `import open_stomp` |

## Development

```sh
cargo test                         # unit tests + tests driven by captures/logs
cargo test --no-default-features   # protocol layer only
cargo clippy --all-targets -- -D warnings
cargo doc --open
nix develop                        # optional dev shell: cargo, rust-analyzer, ALSA, amidi
cd bindings/python && maturin develop && pytest   # Python bindings (pip install maturin pytest)
```

Rust ≥ 1.88. On Linux the `device` feature needs the ALSA headers (`libasound2-dev` / `alsa-lib-devel`). CI runs on
Linux, macOS and Windows.

### Versioning and releases

[SemVer](https://semver.org) (0.x: anything may change). The version lives in one place, `version` in `Cargo.toml`.
To release:

1. Bump `Cargo.toml`, add a `## [x.y.z]` section to [CHANGELOG.md](CHANGELOG.md), update the version line at the top
   of this README and the `tag = "vx.y.z"` in the quick start, and run `cargo build` so `Cargo.lock` follows.
   `scripts/check-version.sh` checks all of this (CI runs it too).
2. Commit, then `git tag vx.y.z && git push --tags`. The release workflow tests the tag and publishes a GitHub
   release with the changelog section as its notes.
3. Point dependents at the new tag (Open Stomp Manager: `OPEN_STOMP_LIB_REF` in its workflows).

## Repository layout

```
src/              the library (see docs/10 for the module map)
bindings/python/  Python bindings (PyO3 + maturin): Rust glue, typed package, tests, examples
examples/         monitor (live / replay), decode (capture → text), ports (list MIDI ports),
                  nam_convert / nam_render / nam_check (NAM conversion, inference, comparisons)
tests/            capture-driven integration tests (fake pedal, engine replay), NAM inference vs
                  NeuralAmpModelerCore (tests/data/nam: its MIT example models + reference outputs)
captures/logs/    capture logs of the official app and the pedal: the test vectors
docs/             the protocol documentation and the library guide
```

The capture/decoding tools and static-analysis artefacts that produced the docs live in the
[open-stomp-manager](https://github.com/JackMechem/open-stomp-manager) repository.

## Legal

This is an independent interoperability project. It is not affiliated with or endorsed by SORAN, NUX/Cherub or
JiYou. Product names are trademarks of their owners. The repo contains **no vendor binaries or firmware**: only
documentation, our own code, and captures of traffic between the author's own pedal and app. Use at your own risk:
writing bad data to a pedal could corrupt presets. Back them up first.

## License

TBD by the repository owner. MIT/Apache-2.0 is suggested for the code and CC-BY-4.0 for the docs.
