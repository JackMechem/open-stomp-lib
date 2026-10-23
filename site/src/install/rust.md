# Use the Rust library

`open-stomp` is not on crates.io yet. Add it from GitHub, pinned to a release tag:

```toml
[dependencies]
open-stomp = { git = "https://github.com/JackMechem/open-stomp-lib", tag = "v0.4.0" }
```

Rust ≥ 1.88. On Linux the `device` feature needs the ALSA headers:

```sh
sudo apt install libasound2-dev      # Debian, Ubuntu
sudo dnf install alsa-lib-devel      # Fedora
nix develop                          # in a checkout: cargo, rust-analyzer, ALSA, amidi
```

macOS and Windows need nothing extra.

## Features

| Feature | Default | What it adds |
|---|---|---|
| `device` | on | `open_stomp::device`, the connection engine (MIDI through midir: ALSA, CoreMIDI, WinMM) |
| `nam` | on | `open_stomp::nam`: run `.nam` models and convert them for the pedal; the optimised IR conversion |

With `default-features = false` you get the protocol layer alone: parsing, building and state tracking, no I/O and
no dependencies.

## First program

Follow everything that happens on the pedal and change the live sound:

```rust
use open_stomp::device::{Config, Device, Update};
use open_stomp::{DeviceState, Module};

fn main() {
    let dev = Device::spawn(Config::default()); // finds and connects to the pedal by itself
    dev.select_preset(4);                       // P05
    dev.set_head(Module::Amp, 8, true);         // AMP on, model 8 (Mar J800)
    dev.set_param(Module::Amp, 0, 100);         // GAIN 100 (live only; nothing is stored until you save)

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
}
```

Import a NAM model and a cab IR into user slots (slots are 0–9; this overwrites what's there):

```rust
dev.import_nam_file(9, "Ampeg SVT.nam")?;      // optimised conversion, about a minute
dev.import_ir_file(9, "Ampeg 8x10.wav")?;      // optimised conversion, under a second
```

## Without a pedal

A checkout includes capture logs of the official app talking to the pedal. The examples replay them:

```sh
git clone https://github.com/JackMechem/open-stomp-lib && cd open-stomp-lib
cargo run --example monitor -- --replay captures/logs/01-main-session.log --speed 8
cargo run --example decode -- captures/logs/01-main-session.log
```

With the pedal: `cargo run --example monitor` (add `--listen` to send nothing) and `cargo run --example ports`.

## Next

* The [library guide](../lib/10-library-guide.md): the engine, state tracking, saving presets, settings, NAM and IR
  imports, your own MIDI transport, replay and testing.
* The [API reference](api.md).
