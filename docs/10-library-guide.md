# 10 — Library guide (`open-stomp`)

The `open-stomp` crate (`use open_stomp::…`) is a Rust library for the SORAN Dual Stomp. It turns docs 02–05 into
code: it parses and builds every message, keeps a model of the pedal up to date from the traffic, paces SysEx
requests the way the pedal needs, and (with the `device` feature) runs the MIDI connection for you.

[Open Stomp Manager](https://github.com/JackMechem/open-stomp-manager) is built on it. Anything the GUI does with
the pedal, your program can do with the calls below. From Python, see [11](11-python-guide.md).

* [Adding it to a project](#adding-it-to-a-project)
* [Layers and modules](#layers-and-modules)
* [Quick start: follow the pedal](#quick-start-follow-the-pedal)
* [The device engine](#the-device-engine)
* [Following state with `DeviceState`](#following-state-with-devicestate)
* [Controlling the pedal](#controlling-the-pedal)
* [Presets and settings records](#presets-and-settings-records)
* [The catalogue](#the-catalogue)
* [Bring your own transport](#bring-your-own-transport)
* [Capture logs and replay](#capture-logs-and-replay)
* [Threading, timing and platform notes](#threading-timing-and-platform-notes)
* [Testing](#testing)
* [Safety](#safety)
* [API reference](#api-reference)

## Adding it to a project

```toml
[dependencies]
open-stomp = { git = "https://github.com/JackMechem/open-stomp-lib", tag = "v0.2.0" }
# or, with a local checkout next to your project:
open-stomp = { path = "../open-stomp-lib" }
# protocol only (no MIDI I/O, no dependencies):
open-stomp = { path = "../open-stomp-lib", default-features = false }
```

| Feature | Default | What it adds |
|---|---|---|
| `device` | on | `open_stomp::device`: the engine thread (midir: ALSA on Linux, CoreMIDI on macOS, WinMM on Windows) |
| `nam` | on | `open_stomp::nam`: run `.nam` models and convert them for the pedal (`serde_json`, `rustfft`) |

Rust ≥ 1.88. On Linux the `device` feature needs the ALSA headers (`libasound2-dev` / `alsa-lib-devel`); macOS and
Windows need nothing extra. The pedal is class compliant, so no driver is needed anywhere.

## Layers and modules

| Module | I/O | What it does | Docs |
|---|---|---|---|
| [`pack`](../src/pack.rs) | none | 8-in-7 bitstream `pack` / `unpack` for SysEx records | [03](03-sysex-protocol.md) |
| [`midi`](../src/midi.rs) | none | `Msg::parse`, `frame`, stream `Assembler`, `req::` reads, `host::` commands, `hex` | [02](02-midi-cc-reference.md), [03](03-sysex-protocol.md) |
| [`catalog`](../src/catalog.rs) | none | `Module`, CC map, model names, `ParamDef`s, drum patterns, note names | [02](02-midi-cc-reference.md) |
| [`records`](../src/records.rs) | none | `Preset` (110 B), `Settings` (17 B), user slot names, ASCII helpers | [04](04-data-structures.md) |
| [`state`](../src/state.rs) | none | `DeviceState`: applies every message, returns a readable `Event` | all |
| [`queue`](../src/queue.rs) | none | `RequestQueue`: one SysEx in flight, reply matching, timeouts, sync progress; `handshake()` | [03](03-sysex-protocol.md) |
| [`upload`](../src/upload.rs) | none | User AMP (NAM) / user IR upload framing: chunks, slot-info record, reassembly | [05](05-uploads-nam-ir.md) |
| [`capture`](../src/capture.rs) | none | Capture log reader | [07](07-capture-methodology.md) |
| [`nam`](../src/nam/mod.rs) | files | `.nam` model loading and inference (NeuralAmpModelerCore port), the NAM → pedal converter | [05](05-uploads-nam-ir.md) |
| [`device`](../src/device.rs) | MIDI | `Device`: hotplug scan, auto-connect, startup sync, edits, replay | this doc |

The crate root re-exports the everyday types: `Module`, `Msg`, `Assembler`, `Preset`, `Settings`, `DeviceState`,
`Event`, `Category`, `Origin`, plus `is_stomp_port` and `USB_ID`.

Everything except `device` is pure: given the same bytes it gives the same result, which is why the capture logs in
`captures/logs/` can be the test vectors for all of it.

## Quick start: follow the pedal

```rust
use open_stomp::device::{Config, Device, Update};
use open_stomp::{Category, DeviceState};

fn main() {
    let dev = Device::spawn(Config::default()); // finds and connects to the pedal by itself
    let mut state = DeviceState::default();
    while let Ok(update) = dev.recv() {
        match update {
            Update::Reset => state = DeviceState::default(),
            Update::Message { origin, bytes, .. } => {
                let ev = state.apply(&bytes, origin);
                if ev.category != Category::Protocol && !ev.redundant {
                    println!("{:8} {} = {}", ev.category.label(), ev.subject, ev.value);
                }
            }
            Update::Info { text, .. } => eprintln!("{text}"),
            _ => {}
        }
    }
}
```

Turn a knob on the pedal and this prints e.g. `AMP      Mar J800 · GAIN = 64`. The full version of this is
[`examples/monitor.rs`](../examples/monitor.rs): `cargo run --example monitor`.

## The device engine

`Device::spawn(config)` starts a background thread and returns a `Device` handle. Talk to it with `Command`s (or the
helper methods) and read `Update`s back. The engine only moves bytes and paces requests; it doesn't interpret them
for you beyond what pacing needs. That is `DeviceState`'s job, in your thread.

```
           Command / helpers                         midir (ALSA / CoreMIDI / WinMM)
 your code ─────────────────▶  engine thread  ◀──────────────────────────────▶ pedal
           ◀─────────────────   ├ port scan (hotplug, 1.5 s)
               Update           ├ Assembler (split SysEx → whole messages)
                                ├ RequestQueue (one SysEx in flight, 1 s timeout)
                                └ capture replay thread
```

### `Config`

| Field | Default | Meaning |
|---|---|---|
| `client_name` | `"Open Stomp"` | MIDI client name shown by the OS |
| `auto_connect` | `true` | Connect to the first pedal-looking port, and again after it is replugged |
| `sync` | `true` | May send anything at all (startup reads and edits). `false` = listen only |
| `reply_timeout` | 1 s | How long a SysEx request waits for its reply/ack before the queue moves on |
| `scan_interval` | 1.5 s | Port rescan period (hotplug detection) |

### Commands

| `Command` | Helper | Effect |
|---|---|---|
| `Connect(port)` | `connect(port)` | Connect to an input port by name (and the matching output); turns auto-connect back on |
| `Disconnect` | `disconnect()` | Close the connection and turn auto-connect off |
| `Resync` | `resync()` | `Update::Reset`, then the full startup read again |
| `SetSync(bool)` | `set_sync(on)` | Allow/forbid sending; off also drops the queue |
| `Send(bytes)` | `send(bytes)` | Send a live CC/PC now, bypassing the queue |
| `Write(msgs)` | `write(msgs)` | Queue messages in order behind pending SysEx; writes wait for their ack |
| `Replay { path, speed }` | `replay(path, speed)` | Disconnect and play a capture log instead of a pedal |
| `StopReplay` | `stop_replay()` | Stop a replay |

Typed helpers built on `send`/`write`: `select_preset(i)`, `set_head(module, model, on)`,
`set_param(module, slot, value)`, `set_volume(v)`, `show_page(module)`, `looper_button(host::LOOP_*)`,
`save_preset(source_slot, &preset)`, `write_settings(&settings)`.

Sends are refused (with an `Update::Info { error: true }`) in listen-only mode or when no output port could be
opened.

### Updates

| `Update` | When |
|---|---|
| `Ports(names)` | At start and whenever the set of MIDI input ports changes |
| `Status(Disconnected / Connected { port, output } / Replaying { file, progress })` | Connection state changes, replay progress |
| `Reset` | A new session starts (connect, resync, replay): reset your `DeviceState` |
| `Sync { done, total }` | Startup sync progress; `(0, 0)` = no sync running (finished, cancelled or disconnected) |
| `Message { at, origin, bytes }` | One complete MIDI message, `Origin::Pedal` (received) or `Origin::Host` (sent by the engine, or by the official app in a replay) |
| `Mark { at, text }` | A `# MARK` annotation in a replayed capture |
| `Info { at, text, error }` | Something to tell the user: connected, unplugged, no reply, refused send |

`at` is a `SystemTime`: arrival time for live traffic, the original capture time for replays.

Receive with `recv()` (blocking), `recv_timeout(d)`, `try_recv()`, or `drain()` (everything available now, for a
render loop). For a GUI, `Device::spawn_with_waker(config, move || ctx.request_repaint())` runs your callback after
each update so the UI wakes up; the engine never touches your UI types.

Dropping the `Device` stops the engine, closes the ports and joins the thread.

### What happens on connect

1. `Update::Reset`, `Update::Status(Connected { … })`.
2. If `sync` is on and an output exists, the startup reads go into the queue (`queue::handshake()`, the official
   app's sequence from [03](03-sysex-protocol.md#startup-handshake-what-the-official-app-does-on-connect-320-ms-total)):
   version, product name, settings, current state, drum, the 10 user IR slots, the 10 user AMP slots, the 99
   presets, then the edit buffer, looper and tuner. 127 requests, each waiting for its reply.
3. `Update::Sync` reports progress after each reply or timeout, ending with `(0, 0)`.
4. From then on the engine forwards everything, and follows up on pedal notifications that carry no data: a preset
   saved on the pedal triggers a re-read of that preset and the edit buffer; toggling the tuner triggers a tuner read.

Presets are read before the edit buffer so `DeviceState` can identify the active slot by matching the edit buffer
against them. No read is known that returns the active slot directly ([09](09-open-questions.md)). If the edit
buffer has unsaved edits the match fails and `state.current` stays `None` until the next preset change.

## Following state with `DeviceState`

`DeviceState::apply(&bytes, origin)` applies one complete message from either direction and returns an `Event`:

| `Event` field | Meaning |
|---|---|
| `category` | `Preset`, `Effect(Module)`, `Volume`, `Navigation`, `Looper`, `Drum`, `Tuner`, `Settings`, `Device`, `Protocol`, `Unknown` (`label()` for display) |
| `subject`, `value` | Plain English, e.g. `"Mar J800 · GAIN"`, `"64"` |
| `coalesce` | A key: consecutive events with the same key are one gesture (a knob sweep) and can be merged in a log |
| `redundant` | Changed nothing (the CC burst after a preset change repeating known values, unused param slots) |
| `raw` | The message bytes |

What it tracks:

| Field | Source |
|---|---|
| `firmware`, `model_id`, `product` | Version / product short frames (`"V1.1.1"`, `"JME-01"`, `"JME STOMPBOX"`) |
| `presets[0..99]` | `0x0B` replies and writes |
| `current` | Program changes; edit buffer matching after sync |
| `live`, `live_known` | The edit buffer: the stored preset plus every CC since (heads, params, volume, rename) |
| `modified()` | `live` sounds different from the stored preset |
| `page` | CC `0x3B` (the pedal's display page) |
| `looper`, `drum`, `tuner` | Their CCs and SysEx records |
| `settings` | `0x14` replies and writes |
| `user_amps`, `user_irs` | NAM / IR slot names (`model_name()` uses them for user models ≥ 30) |

The pedal **never echoes** host messages. The engine reports what it sends as `Origin::Host` messages, so applying
every `Update::Message` keeps `DeviceState` correct for your own edits too. If you send through another path, apply
your bytes yourself.

## Controlling the pedal

Live control is plain MIDI on channel 2 ([02](02-midi-cc-reference.md)): the pedal's sound changes at once, like
turning its knobs, but nothing is stored until you save.

```rust
use open_stomp::Module;
use open_stomp::midi::host;

dev.select_preset(4);                        // P05            C1 04
dev.set_head(Module::Amp, 8, true);          // Mar J800, on   B1 01 08
dev.set_param(Module::Amp, 0, 100);          // AMP GAIN 100   B1 12 64
dev.set_param(Module::Eq, 3, 60);            // EQ band 4 +10
dev.set_volume(80);                          // preset volume  B1 3A 50
dev.show_page(Module::Delay);                // pedal display jumps to DELAY
dev.looper_button(host::LOOP_REC_PLAY_DUB);  // B1 44 01
dev.send(host::cc(0x3F, 1));                 // drum on (any CC from docs/02)
```

`set_head` sets model and on/off together because they share one CC: to switch a block off without changing its
model, pass the current model (`state.live.modules[m.index()].model`).

Parameter slots are the app's display order and map to CCs `first + slot` (`Module::param_ccs()`); slots a model
doesn't use are accepted by the pedal but do nothing. Use `m.params(model)` to know which exist.

### Saving a preset

```rust
let mut p = state.live.clone();      // the edit buffer, with all live edits applied
let source = state.current.unwrap(); // the slot the edits were made on
p.index = 21;                        // store to P22 (or keep `source` to overwrite)
p.name = "Lead".into();              // ≤ 16 printable ASCII; others become '?'
dev.save_preset(source, &p);
```

This queues the official app's exact sequence ([03](03-sysex-protocol.md)): `0x12` name write (first byte = the
source slot), `0x0B` preset write (waits for its ack), then a program change to the saved slot. It is tested byte for
byte against the captured *save as*.

### Importing a NAM model

```rust
dev.import_nam_file(2, "Jcm800.nam")?;            // convert (~4 s, optimised build) + upload to USER AMP 03
```

Or step by step, e.g. to inspect or cache the result:

```rust
use open_stomp::nam::{Model, convert};

let mut model = Model::load("Jcm800.nam")?;
let c = convert(&mut model);                     // c.data: the 2048 floats, c.factor: 1.0
dev.import_user_amp(2, "Jcm800", &c.data, c.factor)?;
```

`Model` also runs the network on your own audio (`model.process(&input, &mut output)`, mono, 48 kHz). The converter
reproduces the official app's algorithm; with our own test signal the resulting filters are within ~1–1.5 dB rms of
the app's (the app's own measurement noise, see [05](05-uploads-nam-ir.md#our-implementation)).
For an exact match with the official app, use its own test signal from an installed Stomp Manager:

```rust
let x = convert::official_excitation("/Applications/Stomp Manager.app")?; // or find_official_excitation()
let c = convert::convert_with(&mut model, &x);                          // = the official conversion
```

This is opt-in; `dev.import_nam_file` never reads the official app.

### Uploading a user AMP or IR

```rust
// data: 2048 converted floats (from nam::convert, or a captured conversion).
// Converting audio files into IR data is not implemented yet (docs/05).
dev.import_user_amp(2, "Jcm800", &data, 1.0)?;   // USER AMP 03: select model 32, slot info, 61 chunks
dev.import_user_ir(0, "My Cab", &data)?;         // USER IR 01: CAB page, model 30, 61 chunks, name
dev.delete_user_amp(2)?;
```

These send the official app's sequences, tested byte for byte against every captured import. They return an
`UploadError` for a slot outside 0–9 or data that isn't 2048 finite floats. `upload::Reassembler` turns captured
chunks back into floats.

### Changing system settings

Settings are written whole. Always start from the record the pedal sent, so unknown bytes survive:

```rust
let mut s = state.settings.clone().expect("synced");
s.input_trim = 13;     // 0–20 = −10…+10 dB
s.loopback = false;
dev.write_settings(&s);
```

Successive settings writes still in the queue collapse into the latest one, so a slider can write on every change.

## Presets and settings records

`Preset::from_packed(payload)` decodes a `0x0B`/`0x0C` body; `to_packed()` encodes it. The decoder keeps the full
110-byte record in `raw` and the encoder writes only known fields back over it, so bytes that are still unknown
([04](04-data-structures.md), [09](09-open-questions.md)) round-trip untouched. When a module's model changes, its
param-count byte is recomputed (`records::param_count`).

| `Preset` field | Record |
|---|---|
| `index` | byte 0 (0-based slot; always 0 in the edit buffer, `DeviceState` fixes it up) |
| `modules[8]: ModuleState { on, model, params[12] }` | heads at 12–19, params per module (indexed by `Module::index()`) |
| `volume` | 78 |
| `chain[7]` | 83–89, module indices of the seven non-gate blocks |
| `name` | 94–109 |

`Settings` decodes `input_trim`, `eq_bass`, `eq_mid`, `eq_treble`, `wireless`, `loopback` and keeps `raw`.
`same_sound()` compares two presets ignoring unused parameter slots.

## The catalogue

```rust
use open_stomp::Module;
use open_stomp::catalog::ParamKind;

let m = Module::Mod;
for (i, name) in m.models().iter().enumerate() {
    let params: Vec<_> = m.params(i as u8).iter().map(|p| p.name).collect();
    println!("#{:02} {name}: {params:?}", i + 1);
}
let p = Module::Cab.param(3, 0).unwrap();     // CAB model 4, slot 0 = MIC
assert!(matches!(p.kind, ParamKind::Enum(_)));
assert_eq!(p.format(4), "SM57");
```

`ParamKind` is `Knob` (0–100), `Bipolar` (0–100, 50 = centre, shown ±) or `Enum(choices)`. `ParamDef::format` gives
the value as the official app shows it. `Module::from_param_cc(cc)` maps a CC back to (module, slot).
`drum_pattern_name(v)` and `NOTES` cover the drum and tuner.

## Bring your own transport

Without the `device` feature (or with a transport midir doesn't cover, such as BLE-MIDI or a web page), drive the
pure pieces yourself:

```rust
use std::time::{Duration, Instant};
use open_stomp::queue::{RequestQueue, follow_up, handshake};
use open_stomp::{Assembler, DeviceState, Msg, Origin};

let mut q = RequestQueue::new(Duration::from_secs(1));
let mut asm = Assembler::default();
let mut state = DeviceState::default();
q.start_sync(handshake());

loop {
    // 1. send whatever the queue releases (nothing while a request is in flight)
    while let Some(r) = q.pop() {
        my_midi_out(r.bytes());
        state.apply(r.bytes(), Origin::Host);
        q.sent(r, Instant::now());
    }
    // 2. feed incoming bytes (any chunking) through the assembler
    for bytes in asm.push(&my_midi_in()) {
        let msg = Msg::parse(&bytes);
        q.on_message(&msg);                  // releases the next request if this answers it
        for r in follow_up(&msg) { q.read(r); }
        state.apply(&bytes, Origin::Pedal);
    }
    // 3. move on from requests the pedal never answered
    q.expire(Instant::now());
}
```

Live CCs/PCs can be sent directly at any time. Put writes through `q.write()` so they don't interleave with a pending
request.

## Capture logs and replay

`capture::parse(text)` reads the log format the sniffer writes ([07](07-capture-methodology.md)):
`<unix ms> <TX|RX> <len> [<port>] <hex…>` lines and `# MARK <ms> <text>` annotations, reassembled into whole
messages as `Entry::Message { ms, origin, bytes }` / `Entry::Mark { ms, text }`.

`dev.replay(path, speed)` plays a log through the engine with its original timing (gaps capped at 1.5 s) divided by
`speed`, emitting the same `Update`s as a live pedal. Use it to develop a front end with no pedal attached:

```sh
cargo run --example monitor -- --replay captures/logs/01-main-session.log --speed 8
cargo run --example decode -- captures/logs/01-main-session.log          # instant, no engine
```

## Threading, timing and platform notes

* One engine thread per `Device`; replays add one short-lived thread. midir's input callback forwards raw bytes to
  the engine over a channel.
* The engine loop ticks every 50 ms when idle; each request goes out as soon as the previous one is answered, as the
  official app does (its whole startup sync takes about 320 ms).
* **macOS:** CoreMIDI only tells a client about hotplugged devices through its creating thread's run loop, so the
  engine turns its run loop once per tick. Without that, a pedal plugged in after start is never seen.
* **Windows:** a MIDI port can be opened by only one program at a time. Quit the official Stomp Manager first.
  Output port names can differ from input names, so the engine falls back to any pedal-looking output.
* **Linux:** ALSA sequencer ports, names like `JME STOMPBOX:JME STOMPBOX MIDI 1 24:0`.

## Testing

```sh
cargo test                       # unit tests + the capture-driven tests below
cargo test --no-default-features # protocol only
cargo clippy --all-targets -- -D warnings
```

| Test | Checks |
|---|---|
| `capture::tests::replay_main_session` | The whole 2 h session applies cleanly: firmware, product, 99 named presets, settings |
| `capture::tests::records_round_trip` | Every preset and settings record on the wire re-encodes byte for byte; param counts agree with the catalogue |
| `capture::tests::save_as_matches_capture` | The captured *save as* is rebuilt from state alone |
| `capture::tests::preset_switch_applies_cached_preset` | Program changes load the cached preset into the edit buffer |
| `tests/offline_pedal.rs` | The full startup sync through `RequestQueue` against a fake pedal answering with the captured replies |
| `tests/device_replay.rs` | The engine thread end to end via replay, including shutdown on drop |
| `queue::tests` | Pacing, timeouts, settings coalescing, follow-up reads |
| `midi::tests`, `records::tests`, `pack::tests`, `catalog::tests` | Builders and codecs against captured bytes |

## Safety

Live CCs are harmless: they change the sound until the next preset change. **Writes are not**: `save_preset` and
`write_settings` overwrite what is stored on the pedal. Preset and settings writes are verified byte for byte against
the captured official app, but have not yet been run on hardware from this library ([09](09-open-questions.md)).
Back presets up (read them, keep the `Preset`s) before experimenting. `Config { sync: false, .. }` guarantees nothing
is ever sent.

## API reference

`cargo doc --open` builds the full API documentation from the doc comments.
