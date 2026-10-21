# Changelog

All notable changes to `open-stomp`. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- `nam` feature and module (on by default): load and run Neural Amp Modeler `.nam` models (a
  pure-Rust port of NeuralAmpModelerCore's WaveNet, plain and `SlimmableContainer`; matches it to
  ≤ 3e-5 on its example models, which are now test vectors), and `nam::convert`, the official
  app's NAM → pedal converter: a fundamental/3rd-harmonic tone test and a small-signal linear
  response give two minimum-phase FIRs. With the app's own excitation the result matches its
  upload to ≤ 0.03 dB; with ours (the default) within ~1–1.5 dB rms, the app's own measurement
  noise. `Device::import_nam_file(slot, path)` converts and uploads. Examples `nam_convert`,
  `nam_render`, `nam_check`. Python: `open_stomp.nam`, `Device.import_nam_file`.
- `upload` module: user AMP (NAM, SysEx `0x58`) and user IR (`0x10` + `0x09`) upload framing:
  chunking, the 128-byte slot-info record (`UserAmpInfo`: name, factor), delete, and a
  `Reassembler` that turns captured chunks back into floats. Every captured import rebuilds
  byte for byte. `Device::import_user_amp` / `delete_user_amp` / `import_user_ir` send the
  official app's sequences (2048 finite floats, slots 0–9, else `UploadError`). Also in the
  Python bindings (`open_stomp.upload`, `Device.import_user_amp` …).
- `DeviceState` follows user AMP imports and deletes (slot names, log events with progress).

### Changed
- docs/05: corrected chunk header (plain byte offset + total length, not 8-byte units), the
  slot-info record layout, and the app selecting the slot's model *before* uploading.
- docs/05, docs/09: the NAM converter algorithm, recovered from the vendor code and verified stage
  by stage: a small-signal best linear approximation and a sine tone test (fundamental and 3rd
  harmonic) through the network give two minimum-phase FIRs (Wiener–Hammerstein G1/G2) whose first
  1024 taps, reversed, are the upload; `factor` is always 1.0.

## [0.2.0] - 2026-10-21

### Added
- **Python bindings** (`bindings/python`, `pip install ./bindings/python`, `import open_stomp`):
  the whole library through PyO3, mirroring the crate's modules: protocol, catalogue, records,
  `DeviceState`, `RequestQueue`, capture reader and the `Device` engine. One `abi3` wheel per
  platform for Python ≥ 3.9, typed stubs checked with `stubtest`, GIL released while waiting,
  Ctrl-C interrupts blocking calls. `DeviceState.feed(update)` applies engine updates. Tests
  driven by the capture logs, `monitor.py` and `shell.py` (interactive) examples and a Python
  guide (docs/11).
- The repository is now a Cargo workspace; the crate and the Python package share its version.

## [0.1.0] - 2026-10-21

First release as a standalone library. The protocol code comes from the `stomp-proto` crate of
[Open Stomp Manager](https://github.com/JackMechem/open-stomp-manager) 0.3.0, and the connection engine from that
app's `device` module.

### Added
- Protocol layer (no I/O, no dependencies): 8-in-7 packing, message parsing and SysEx framing,
  stream reassembly, request (`req::`) and command (`host::`) builders, the module / model /
  parameter catalogue, preset and settings records (decode and encode, unknown bytes preserved),
  the `DeviceState` tracker with plain-English events, and the capture log reader.
- `queue::RequestQueue`: the pedal's one-request-at-a-time pacing as a pure state machine (reply
  matching, timeouts, sync progress, settings-write coalescing), `queue::handshake()` (the official
  app's startup reads) and `queue::follow_up()` (re-reads after pedal notifications).
- `device` feature (default): `Device` engine thread with port scanning and hotplug, auto-connect,
  startup sync, live sends, queued writes, listen-only mode and capture replay. Typed helpers for
  presets, models, parameters, volume, pedal page, looper, preset save and settings writes. Clean
  shutdown when the handle is dropped. GUI-agnostic: a wake callback replaces the app's egui
  dependency, and timestamps are `SystemTime`.
- `is_stomp_port`, `USB_ID`.
- Examples: `monitor` (live or replayed, decoded), `decode` (capture to text), `ports`.
- Tests: the startup sync against a fake pedal answering from the capture, and the engine thread
  end to end through replay, on top of the existing capture-driven tests.
- Documentation: the full protocol docs (01–09) and a library guide (10).

[Unreleased]: https://github.com/JackMechem/open-stomp-lib/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/JackMechem/open-stomp-lib/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/JackMechem/open-stomp-lib/releases/tag/v0.1.0
