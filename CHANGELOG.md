# Changelog

All notable changes to `open-stomp`. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Bluetooth (the pedal's Wireless setting), feature `ble`: the engine also scans for the pedal over BLE-MIDI
  (btleplug: CoreBluetooth, BlueZ, WinRT) and lists it as a port `"JME STOMPBOX (Bluetooth)"`, with no pairing.
  It works like USB: confirmed on the pedal for the full startup sync, effect on/off, parameters, volume, page,
  preset switching, preset and settings writes, drum and looper (`examples/ble_check.rs`), and NAM and IR imports
  (about 10 s each; the official app doesn't offer imports over Bluetooth). Auto-connect prefers USB
  and moves to USB when the pedal is plugged in. `Config::bluetooth` (default on) turns it off; Python
  `Device(bluetooth=...)`, which is built with `ble`. `is_ble_port`, `BLE_PORT_SUFFIX`.
- `blemidi`: BLE-MIDI packet framing (encode with packing and SysEx splitting, decode with running status).

### Changed
- Messages to a Bluetooth pedal go out at least 15 ms apart, and a CC replacing the one queued just before it
  overwrites it: an unpaced burst of 61 CCs froze the pedal until a power cycle.
- On Linux, `ble` needs the D-Bus headers (`libdbus-1-dev`); CI and the Linux wheel install them.

### Fixed
- The edit-buffer read (`0x0C`) is padded with 14 zero bytes like the official app's. A bare request made the pedal
  take its parameters from whatever the previous message left in its buffer, giving empty or truncated replies.

## [0.5.1] - 2026-10-22

### Added
- Rename user slots without touching their sound: `Device::rename_user_amp(slot, name)` and
  `Device::rename_user_ir(slot, name)` (Python too) send only the import's name record (the AMP slot-info write,
  the IR `0x09` name write) and re-read the slot. The official app can't rename; confirmed on the pedal (a slot's
  sweep response before and after differs by at most 0.09 dB, rig noise; docs/05, "Renaming a slot"). Names are cut
  to 31 characters.

### Changed
- The activity event for a user AMP slot-info write is "User AMP slot" (it was "User AMP import"), since a rename
  sends the same record; like "User IR slot".
- README and docs site: the NAM and IR graphs re-measured on hardware in one session with the official app's, 0.4's
  and 0.5's conversions (`tools/pedalsim/readme_plots.py <session>`, now with a dry-take reference instead of the
  Mac rig's calibration); the real-playing comparison is on the README and the site's front page.

## [0.5.0] - 2026-10-22

### Added
- Documentation site (mdBook, `site/`, published to GitHub Pages by `.github/workflows/pages.yml`): install guides,
  both repositories' docs, the NAM / IR analysis with an interactive re-amp comparison, and the API reference.

### Changed
- `nam::optimise`: the loss now weights frequencies so each octave counts about the same
  (`Options::weighting`, `Weighting::Octave`, the new default). The old flat weighting gave the bottom octave
  ~0.5% of the fit, and imports came out 5–10 dB short at 50–160 Hz. In simulation the low-end error (40–160 Hz)
  of the Ampeg SVT-VR Classic (Normal) model drops from 4.9 to 2.1 dB rms and the frequency response error from
  2.5 to 1.2 dB; the frequency response improves on all six models tried, the low end on five (docs/05, "Low
  end"). `Weighting::Flat` keeps the old loss;
  example `nam_optimise --flat`. On hardware, a bass DI re-amped through the pedal against the
  model and IR in software: low end 5.2 → 3.8 dB rms, overall tone 3.5 → 2.8 dB (2.1 dB with the IR change below).
- `ir::convert_optimised`: the tap fit now covers 30 Hz – 16 kHz instead of 25 Hz – 20 kHz. Cab IRs have next to
  nothing above 16 kHz, and matching it cost taps the rest of the range needs: on fifteen Ampeg IRs the 1/3-octave
  error of what the pedal plays drops from 2.42 to 2.18 dB rms (better on 13; the two DI recordings get slightly
  worse), and the 40–160 Hz error from 3.55 to 2.90 dB.

## [0.4.0] - 2026-10-22

### Added
- NAM import level normalisation, like NAM plugins: `nam::convert::normalise_level` scales G2 (after the pedal's
  saturation, so a pure volume change) so that every model plays at the same level, `NORMALISED_LEVEL_DB`,
  measured with `pedal_level_db` on pink noise and calibrated on the pedal to its built-in bass amps (two SVT-VR
  models 6 dB apart raw land within 0.2 dB of each other; un-normalised they sat 13–19 dB below the built-in amps). Models span about 17 dB raw; the conversions inherit
  the official app's level, which plays models quietly (24–34 dB below their own output). On by default for
  `Device::import_nam_file`; `Device::import_nam_file_as(.., normalise)`, Python `nam.convert(..., normalise=True)`
  and `Device.import_nam_file(..., normalise=...)`. `nam::Model::loudness` (the file's metadata). Example
  `nam_level`.
- `ir` module: read and write WAV files (`Wav`: PCM 8–32-bit and float, any channel count), and `convert`, the
  official app's IR conversion recovered from its native code: files that aren't 48 kHz mono 24-bit are converted
  first (first channel, JUCE's resampler, `resample_like_app`, 24-bit), then the first 2048 samples are reversed and
  scaled so the 398 Hz – 3 kHz response is centred on 0 dB (`app_gain`). Identical to eleven captured imports
  (`tests/ir_import.rs`). `Device::import_ir_file(slot, path)` converts and uploads, named like the app (first 16
  characters). Also a windowed-sinc `resample`. Python: `open_stomp.ir`, `Device.import_ir_file`. Examples
  `ir_check`, `ir_test_files` (designed test IRs, `tests/data/ir`) and `ir_capture_check`.
- The pedal plays only the first 384 taps of an uploaded IR (re-amped probe IRs; `ir::PEDAL_IR_TAPS`, docs/05).
- `ir::convert_optimised` / `Method::Optimised` (feature `nam`): a 384-tap minimum-phase filter fitted to the whole
  IR's 1/6-octave response, at the level the app means to give it. On fifteen bass IRs in software, what the pedal
  plays is 2.4 dB rms from the full IR against 7.9 dB for the official conversion, which leaves IRs with a delayed
  start 35–44 dB too quiet. Confirmed on hardware (re-amped: 2.1 vs 4.8 dB rms, and 5.5 dB vs 16 dB rms plus 46 dB
  too quiet for a delayed IR). The default for `Device::import_ir_file` (Python too); `convert_with`,
  `Device::import_ir_file_with(.., Method::App)` for the official conversion, Python `method=`, example `ir_compare`.

### Fixed
- User IR uploads put the slot in data-header byte 0, as the official app does (it was in byte 1, NAM's slot byte,
  so only slot 0 was right). `upload::Chunk::ir_slot`; `Reassembler` reports either slot.

## [0.3.0] - 2026-10-21

### Added
- `nam` feature and module (on by default): load and run Neural Amp Modeler `.nam` models (a
  pure-Rust port of NeuralAmpModelerCore's WaveNet, plain and `SlimmableContainer`; matches it to
  ≤ 3e-5 on its example models, which are now test vectors), and `nam::convert`, the official
  app's NAM → pedal converter: a fundamental/3rd-harmonic tone test and a small-signal linear
  response give two minimum-phase FIRs. With the app's own excitation the result matches its
  upload to ≤ 0.03 dB; with ours (the default) within ~1–1.5 dB rms, the app's own measurement
  noise. `Device::import_nam_file(slot, path)` converts and uploads. Examples `nam_convert`,
  `nam_render`, `nam_check`. Python: `open_stomp.nam`, `Device.import_nam_file`. Confirmed on
  hardware: an official and an open conversion of the same bass model sound identical.
- `nam::convert::Measure::Clean` (`convert_measured`; Python `nam.convert(..., measure="clean")`),
  experimental: the same converter with a steady-state small-signal measurement instead of the
  app's transient-affected one. Re-amp tests on hardware against the real model (Ampeg SVT bass
  model): closer mid scoop, top-end roll-off and upper-note distortion than the official
  algorithm, but still short on the 63–90 Hz resonance and about 5 dB quieter. Not the default.
- `nam::convert::PEDAL_TAPS`, `upload_data`, `pedal_irs`: the pedal runs only the first 512 taps
  of each uploaded filter (found by re-amping; docs/05). `Measure::Clean` now uploads exactly
  those taps, with the ignored halves zeroed; `Measure::App` keeps the app's layout.
- `nam::optimise` and `Measure::Optimised`, the default for `Device::import_nam_file` (about a
  minute per model): fit G1/G2 so a simulation of the pedal (`y = G2 ⊛ f(G1 ⊛ x)`, its saturation
  curve fitted from re-amp recordings) matches the model on sweeps, tones and pink noise, starting
  from the clean conversion. Hand-written gradients, Adam, no ML framework. On hardware (Ampeg SVT
  bass model) it scores 6.72–6.85 dB rms error against the real model, against 9.32 for the
  official conversion in the same session. `Device::import_nam_file_with(.., Measure::App)` keeps
  the official algorithm (a few seconds). Progress and cancel: `convert::convert_optimised`,
  `optimise::optimise_with_progress`. Also `optimise::simulate`, example `nam_optimise`.
  Python: `Device.import_nam_file(..., measure="optimised")` (default) and `nam.convert(...,
  measure="optimised")`.
- `nam::convert::official_excitation` / `find_official_excitation`: read the official app's test
  signal from the user's own Stomp Manager install (located by fingerprint; no vendor data in the
  source). With it the conversion equals the official one (error energy −51 to −93 dB on ten
  models). Strictly opt-in: `Device::import_nam_file` and everything else never read the app.
  Python: `nam.official_excitation()`.
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

[Unreleased]: https://github.com/JackMechem/open-stomp-lib/compare/v0.5.1...HEAD
[0.5.1]: https://github.com/JackMechem/open-stomp-lib/compare/v0.5.0...v0.5.1
[0.5.0]: https://github.com/JackMechem/open-stomp-lib/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/JackMechem/open-stomp-lib/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/JackMechem/open-stomp-lib/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/JackMechem/open-stomp-lib/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/JackMechem/open-stomp-lib/releases/tag/v0.1.0
