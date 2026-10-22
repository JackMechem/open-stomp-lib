# Designed test IRs

Written by `examples/ir_test_files.rs`. Each isolates one question about the official app's IR conversion
(docs/05). They were imported into USER IR 01 … 10 with the official app while capturing
(`captures/logs/02-ir-tests.log`); `tests/ir_import.rs` checks our conversion and upload against every import, and
`examples/ir_capture_check` prints the comparison.

| File | Content | Question |
|---|---|---|
| T1 impulse | 48 kHz, 2048 samples, 0.5 at 0 | the gain for the simplest IR |
| T2 two impulses | + −0.25 at 1000 | gain rule: peak vs energy vs sum |
| T3 tail | 4800 samples, 0.5 at 0, 0.9 at 3000 | does the gain look past sample 2048? |
| T4 quiet | 0.01 at 0 | is the level normalised away? |
| T5 44k | 44.1 kHz, 0.5 at 100 | the resampler (shape of the interpolated impulse) |
| T6 stereo | L: 0.5 at 0, R: −0.5 at 50 | which channel, or a mix |
| T7 short | 100 samples | padding of short IRs |
| T8 float | 32-bit float, 0.5 at 0, 0.3 at 10 | float WAV input |
| T9 a name longer than sixteen | 0.25 at 0 | name truncation, and scale vs T1 |
| T10 96k | 96 kHz, 0.5 at 200 | downsampling |
