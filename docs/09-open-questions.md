# 09 — Open questions

What is still unknown about the pedal, ordered by how much it matters for an editor. Each item says how it could be
answered. Most need a capture of the official app ([07](07-capture-methodology.md)) or an experiment on the pedal.
If you find an answer, please open an issue or a pull request that updates this page.

## High value

1. **Preset unknown bytes**: offset 8 (`0x62`), 79–81, 82 (`6`), 90–93. Toggle things in the app that aren't
   covered yet (chain reordering, tempo/BPM per preset, scenes if any) and diff `0x0C` reads.
2. **Host→pedal CCs not yet captured**: loop mode `0x4A` and tuner control. (Drum pattern `0x40` and loop levels
   `0x42`/`0x43` are confirmed, see [02](02-midi-cc-reference.md).)
3. **Writing the edit buffer** (`0x0C` dir 1): does it exist? The app only uses live CCs and the `0x0B` save.
4. **Preset and settings writes on hardware**: the library's writes match the official app byte for byte, but
   haven't been run on a pedal from this code yet.
5. **IR slot 4-byte field** (`0x09` record bytes 0–3): `01 00 00 00` in all eleven captured imports (ten slots,
   different files and names), so probably a constant or a "slot in use" flag.

## Medium

6. **Settings unknown bytes** 0, 2 (30), 3 (10), 7–8 (50, 50), 11 (60), 12 (180). Candidates: energy-saving mode,
   USB audio routing (Normal / LoopBack / Reamp / Dry-Wet per static strings), display, MIDI channel.
7. **Value → unit curves**: CAB LOW/HIGH CUT raw → Hz, EQ raw → dB, tuner cents scale, delay TIME → ms, Hall
   pre-delay. Read the app's displayed value at several raw values, or find the tables in the snapshot (the static
   analysis found log-spaced "20…1000" and "5000…20000" string tables).
8. **Current-state `0x15`** fields (`50 01 …`).
9. **Unsolicited `F0 00 21 7A 10 F7`**, seen twice mid-session.
10. **Delta CC dumps**: confirm that the CC burst after a program change only contains changed values, by switching
    between two presets that differ in one parameter.
11. **LSTM and ConvNet NAM models**: the app's NAM core supports ConvNet too; the library converts WaveNet models
    only.

## Low / out of scope

12. **Bluetooth pacing**: 15 ms between messages works; the fastest safe pacing is unknown.
13. **The official app's import/export/backup file formats** (`.bin`, the preset import list dialog). Not captured.
14. **Firmware DFU over USB HID** (separate updater). Not explored here.
15. **The legacy "Syx/Cherub" header** `43 58 70 7E` seen in static analysis: never observed from this pedal.

## Resolved

- **NAM → device conversion**: recovered and implemented (`open_stomp::nam`, [05](05-uploads-nam-ir.md#our-implementation)).
  An Ampeg SVT model converted by the official algorithm and by ours sounds identical on the pedal.
- **IR transform**: resampled first channel, first 2048 samples reversed, gain centring the 398 Hz–3 kHz response on
  0 dB. `ir::convert` matches eleven captured imports. The pedal plays only the first 384 taps ([05](05-uploads-nam-ir.md)).
- **`0x12` set-name first byte**: the active preset (P69 = `0x44` was selected just before the captured save).
- **BLE-MIDI**: the same messages as over USB, NAM and IR uploads included ([01](01-overview.md)).
