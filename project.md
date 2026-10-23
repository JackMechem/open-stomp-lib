# project.md: open-stomp (protocol library + docs)

## Goal

A standalone Rust library (with Python bindings) that speaks the SORAN Dual Stomp Lite protocol: protocol
encode/decode, device state, request queue and connection engine, upload transport, and the NAM / IR conversions
the official *Stomp Manager* does. It is also the home of the protocol and reverse-engineering documentation.

The desktop editor, [open-stomp-manager](https://github.com/JackMechem/open-stomp-manager)
(`~/Projects/open-stomp/open-stomp-manager`), depends on this crate by path and has its own project.md. The
reverse-engineering workbench (sniffer, decoders, static-analysis artefacts) also lives there.

## How the protocol was recovered

1. **Static analysis** of Stomp Manager 1.0.2 (macOS DMG / installed app): Flutter app, Dart 3.12.2 AOT
   (obfuscated). Object pool dumped with a patched blutter; whole `__text` disassembled and annotated; native libs
   (`nux_plugins` = JUCE audio converter, `imagelib` = NeuralAmpModelerCore) and firmware container inspected.
   → [docs/08](docs/08-static-analysis.md)
2. **Live capture** by hooking CoreMIDI inside an ad-hoc re-signed copy of the app (bus-level USB sniffing isn't
   possible on Apple Silicon without disabling SIP). Every pedal and app action was performed one at a time, marked
   in the log, and the app window screenshotted and OCR'd to map model numbers to names.
   → [docs/07](docs/07-capture-methodology.md)
3. **Decoding**: CC map per module, SysEx framing, the 8-in-7 packing, the preset layout (by correlating 99 preset
   records with CC dumps), settings, the looper state machine, and the upload transport.
   → [docs/02](docs/02-midi-cc-reference.md)–[05](docs/05-uploads-nam-ir.md)

## Key facts (cheat sheet)

* USB `3962:2000`, class-compliant UAC2 + USB-MIDI. No vendor interface.
* Pedal → host: ch 1. Host → pedal: ch 2. Same CC numbers both ways.
* Head CC per module = `(off?0x40:0) | model`: OD/FX `0x00`, AMP `0x01`, CAB `0x02`, MOD `0x03`, DLY `0x04`,
  RVB `0x05`, GATE `0x06`, EQ `0x07`. Params: OD/FX `0x0C–0F`, GATE `0x08–09`, AMP `0x12–17`, CAB `0x1A–1E`,
  EQ `0x20–2B`, MOD `0x2C–2F`, DLY `0x30–33`, RVB `0x36–39`. VOL `0x3A`, page `0x3B`.
* Presets: PC index 0–98 (= P01–P99).
* SysEx: `F0 00 21 7A 70 <cmd> <dir> … F7`, dir 0 read / 1 write / 2 reply / 3 ack; `7E` = notify.
  cmds: 09 IR slot, 0B preset, 0C edit buffer, 10 IR data, 12 name, 14 settings, 15 state, 18 loop, 19 drum,
  58 user amp/NAM, 6F tuner.
* Packed records: 7-bit LSB-first bitstream (126 MIDI bytes ↔ 110 struct bytes for a preset).
* The pedal plays only the first 384 taps of an uploaded IR (`ir::PEDAL_IR_TAPS`).

## Roadmap

- [x] Static analysis
- [x] Capture + decode all pedal-side controls
- [x] Capture + decode app-side live control, rename/save, settings, uploads (transport)
- [x] Documentation (docs 01–11) + library guide
- [x] Protocol layer: pure encode/decode + tests against `captures/logs`
- [x] State, request queue, connection engine (`Device`)
- [x] Python bindings (`bindings/python`, `import open_stomp`)
- [x] NAM → device conversion: official algorithm recovered, plus an optimised conversion (verified on hardware)
- [x] IR → device conversion: official algorithm recovered (byte-exact vs eleven captured imports), plus an
      optimised 384-tap conversion (verified on hardware)
- [ ] Open questions that matter for v1 ([docs/09](docs/09-open-questions.md) #4–#6): #5 mostly confirmed on
      Linux (host `0x40`/`0x42`/`0x43`); `0x4A`, tuner control, #4 and #6 still open
- [ ] Run preset/settings *writes* on hardware (byte-exact vs the capture, not yet run from this code)
- [ ] LSTM / ConvNet NAM models
- [ ] `stompctl` CLI (info, monitor, preset get/set/backup; see [docs/06](docs/06-rust-linux-guide.md))

## Environment notes

* Pedal firmware during captures: V1.1.1. Stomp Manager: 1.0.2+1.
* `tools/pedalsim/` holds the Python analysis behind the optimised NAM/IR conversions (re-amp rig, pedal
  simulator, scoring).
* New captures are made with the workbench in open-stomp-manager (`tools/sniffer`, see its project.md); copy logs
  that become test vectors into `captures/logs/`.
