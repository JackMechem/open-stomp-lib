# Changelog

All notable changes to `open-stomp`. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[Unreleased]: https://github.com/JackMechem/open-stomp-lib/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/JackMechem/open-stomp-lib/releases/tag/v0.1.0
