# 09 — Open questions

Ordered by usefulness for a Linux editor. Each item says how to resolve it.

## High value

1. ~~**NAM → device conversion**~~: solved and implemented (`open_stomp::nam`, [05](05-uploads-nam-ir.md#our-implementation)).
   Confirmed on hardware (2026-10-21, firmware V1.1.1): an Ampeg SVT model converted by the official algorithm and by
   ours, both uploaded by `open_stomp`, sound identical on the pedal. Left: LSTM/ConvNet NAM models (the app's NAM
   core supports ConvNet too).
2. ~~**IR transform**~~: solved (docs/05): resampled first channel, first 2048 samples reversed, gain centring the
   398 Hz – 3 kHz response on 0 dB. `ir::convert` matches eleven captured imports. The pedal plays only the first
   384 taps (docs/05).
3. **IR slot 4-byte field** (`0x09` record bytes 0–3): `01 00 00 00` in all eleven imports captured (ten slots,
   different files and names), so probably a constant or a "slot in use" flag.
4. **Preset unknown bytes**: offset 8 (`0x62`), 79–81, 82 (`6`), 90–93. Toggle things in the app that aren't
   covered yet (chain reordering, tempo/BPM per preset, scenes if any) and diff `0x0C` reads.
5. **Host→pedal CCs not yet captured**: ~~drum pattern `0x40`, loop levels `0x42/0x43`~~ (confirmed on Linux, see
   [02](02-midi-cc-reference.md)); loop mode `0x4A` and tuner control remain.
6. **Writing the edit buffer** (`0x0C` dir 1): does it exist? (The app only did live CCs + `0x0B` save.)

## Medium

7. **Settings unknown bytes** 0, 2 (30), 3 (10), 7–8 (50, 50), 11 (60), 12 (180). Candidates: energy-saving mode,
   USB audio routing (Normal / LoopBack / Reamp / Dry-Wet per static strings), display, MIDI channel.
8. **Value → unit curves**: CAB LOW/HIGH CUT raw → Hz, EQ raw → dB, tuner cents scale, delay TIME → ms, Hall
   pre-delay. Read the app's displayed value at several raw values, or find the tables in the snapshot (the static
   analysis found log-spaced "20…1000" and "5000…20000" string tables).
9. ~~**`0x12` set-name first byte**~~: it is the active preset (P69 = `0x44` was selected just before the save in
   the capture). The app sends the same.
10. **Current-state `0x15`** fields (`50 01 …`).
11. **Unsolicited `F0 00 21 7A 10 F7`** seen twice mid-session.
12. **Delta CC dumps**: confirm that the post-PC CC burst only contains changed values, by switching between two
    presets that differ in one parameter.

## Low / out of scope

13. BLE-MIDI: same messages over BLE? (Service/characteristic UUIDs are known.)
14. Import/export/backup file formats of the official app (`.bin`, preset import list dialog). Not captured.
15. Firmware DFU over USB HID (separate updater). Deliberately not explored.
16. The legacy "Syx/Cherub" header `43 58 70 7E` seen in static analysis: never observed from this pedal.
