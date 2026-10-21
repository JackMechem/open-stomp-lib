# 05 — User AMP (NAM) and User IR uploads

Status: **transport fully decoded and implemented** (`open_stomp::upload`, rebuilt byte for byte from every captured
import); **content conversion NOT yet reversed.** You can upload any 2048 floats (e.g. a captured conversion) to any
slot, but generating them from a `.nam` / `.wav` file still requires reversing the app's converter (open problem #1).

## Common chunk format

Both uploads send a series of SysEx writes, each acknowledged (`dir` 3, echoing the header) before the next. The
body after `<dir>` is one **packed** stream ([03](03-sysex-protocol.md#2-packed-8-in-7-bitstream-packed)):

```
unpacked = 12-byte header | data …

header:  [0] 0   [1] slot   [2] sub-command   [3] 0   [4..8] byte offset (u32 LE)   [8..12] total length (u32 LE)
```

* Data chunks carry **136 bytes = 34 float32** (little-endian); the last one is shorter. Offsets are plain byte
  offsets: 0, 136, 272 … 8160.
* **Every upload so far is exactly 2048 float32 = 8192 bytes** (`total` = 8192), in 61 chunks (60 × 136 + 32 bytes).
* The same header carries the slot-info record (`total` = 128) and header-only requests (read info, delete).
* The real transfer takes only ~100 ms. The long wait in the app is the conversion.
* Implemented as `open_stomp::upload` (`Chunk`, `data_frames`, `Reassembler`, `UserAmpInfo`, `user_amp_upload`,
  `user_ir_upload`) and `Device::import_user_amp` / `delete_user_amp` / `import_user_ir`.

[`tools/decode/nam_extract.py`](https://github.com/JackMechem/open-stomp-manager/blob/main/tools/decode/nam_extract.py) (in the open-stomp-manager repo) extracts the floats from a capture log:

```
python3 tools/decode/nam_extract.py captures/logs/01-main-session.log "<marker text>" out.bin
```

## User AMP / NAM — SysEx `0x58`

Unpacked header byte 1 = slot (0–9 → USER AMP 01–10, amp models 30–39); byte 2 = sub-command:

| Sub (unpacked) | MIDI byte 2 | Meaning |
|---|---|---|
| 1 | `04` | data chunk |
| 3 | `0C` | write slot info (128-byte record) |
| 4 | `10` | read slot info (reply: the 128-byte record) |
| 6 | `18` | delete slot |

Captured import (Fender Super Reverb NAM → USER AMP 01):

```
→ B1 01 1E                        select amp model 30 (USER AMP 01), before the upload
→ 70 58 01 <info, total 128>      write slot info      ← 70 58 03 <header> ack
→ 70 58 01 <data, offset 0>       data chunk 0         ← ack (echoes header)
→ … 61 data chunks total, offsets 0, 136 … 8160 …
```

The app does not re-read the slot after an import. Delete: `→ 70 58 01 00 00 18 00…` `← 70 58 03 <1 byte>`, then the
app re-reads the slot info, which comes back as an empty slot.

### Slot-info record (128 bytes)

| Bytes | Meaning |
|---|---|
| 0–31 | Name, ASCII, zero-padded (the app sends the file name without `.nam`, up to 31 characters) |
| 32–35 | `u32` = 256 in every record seen (empty or imported) |
| 36–51 | 0 |
| 52–55 | `f32` **factor**: 1.0 in both captured imports (FNDR, Jcm800). Probably the `factor` of the converter callback |
| 56–95 | 0 after an import; an empty slot reads back with `1.0` in each of the ten floats 56–95 too |
| 96–127 | 0 |

An empty slot reads back as name `"USER AMP NN"`, the `u32` 256 and eleven `1.0`s at 52–95. Both imports and the
re-import were byte-identical apart from the name.

### What the 2048 floats are

The pedal does **not** run NAM natively. The app's native library (`imagelib`, which bundles NeuralAmpModelerCore)
loads the `.nam` and converts it. Findings:

* The test files are NAM 0.7 `SlimmableContainer`s with two WaveNet submodels (3-channel: 1871 weights; 8-channel:
  12146 weights). The uploaded floats are **not** those weights (searched all alignments and float16/32).
* The 2048 floats split into two halves:
  * `[0..1023]` rises smoothly and exponentially to a large value (~2200): looks like a **lookup table / transfer
    curve**, or a time-reversed filter.
  * `[1024..2047]` is tiny, with energy rising toward the end: looks like a **time-reversed impulse response**.
* The static analysis shows `imagelib` callbacks deliver `(float* g1, float* g2, float* factor)`. So the converter
  most likely approximates the network as **filter(s) + static nonlinearity** (a Wiener/Hammerstein-style model):
  g1, g2 = two 1024-float arrays, factor = a scalar that probably sits in the slot-info record (sub 3).
* **Deterministic:** importing the same file twice produced byte-identical info and data, so captures are valid
  test vectors.

Test vectors (not in git; see `.gitignore`. Regenerate from the logs):

| Input file | Log marker | Output |
|---|---|---|
| `FNDR BFSR VB Edge WRM2 CAB FREE.nam` | `NAM import 'FNDR` | `captures/files/FNDR.device_floats.bin` |
| `Jcm800.nam` (from "Marshall Jcm800 + V30 1960.zip") | `deleted USER AMP NAM` (the next import after it) | `captures/files/Jcm800.device_floats.bin` |

### The converter, run headless (2026-10)

The app's converter can be run outside the app: `tools/namconv/` in the
[open-stomp-manager](https://github.com/JackMechem/open-stomp-manager) repo (`nam-re` branch) loads
`imagelib.framework` from the installed app, calls `ImageUtility::loadNamFile` by address (the C++ API isn't exported,
but the symbol table names everything and `__TEXT` starts at 0), waits for the end callback and dumps every buffer.
No pedal is involved. For both test files the harness output is **byte-identical** to the captured upload, so it is
a reliable reference implementation, intermediate buffers included.

What the buffers show (FNDR and Jcm800):

| Buffer | Size | Finding |
|---|---|---|
| `ToneTest_FftBins` (const) | 94 × `int32` | Tone frequencies as bins of an **8192-point FFT at 48 kHz**: 10…2488 = 58.6 Hz…14.6 kHz, ~1/12-octave steps |
| `ToneTest_FftBinsThird` (const) | 83 × `int32` | Exactly **3 × the fundamental bins**: the 3rd harmonic of each tone (up to 3951 < 4096) |
| `ToneTestOutputLevels` | 94 × `f32` | Level of each tone's fundamental at the model's output |
| `ToneTestOutputLevelsThird` | 83 × `f32` | Level of each tone's 3rd harmonic |
| `g1GainsDb` | 94 (+1) × `f32` | Target gain per tone for G1, in dB. Roughly *constant − fundamental level*: G1 may be close to the inverse of the measured response (to check) |
| `G1IR`, `G2IR` | 8192 × `f32` | Two impulse responses. `|FFT₈₁₉₂(G1IR)|` at the tone bins equals `g1GainsDb` (mean error 0.01–0.13 dB); both peak at sample 1–6 (minimum-phase-like; `Magnitude2MinPhase` and `hilbert2` exist) |
| `NamFirG1`, `NamFirG2` → upload | 2 × 1024 × `f32` | **`g1 = reverse(G1IR[0:1024])`, `g2 = reverse(G2IR[0:1024])`**: the upload is the first 1024 taps of each response, time-reversed. That is the "smoothly rising" first half seen in the captures |
| `factor` | `f32` | 1.0 for both files; written into the slot-info record (offset 52) |

Working model: the app identifies a **Wiener–Hammerstein** approximation of the network, *G1 → static nonlinearity →
G2*, by playing sine tones through the NAM model and measuring each tone's fundamental and 3rd harmonic. It then
designs G1 and G2 as minimum-phase FIRs and the pedal runs them around its own fixed nonlinearity. What remains:

1. How a tone is generated and measured (amplitude, length, window; `getSpectrum`, `ampCapture_run_data`,
   `levelsTesterItem`).
2. How `g1GainsDb` is derived from the levels (and what the 95th value is), and how G2 is derived.
3. The magnitude → minimum-phase design (`Magnitude2MinPhase`, `hilbert2`, `FIRExecObj_compile`) and the
   interpolation between tone bins.
4. When `factor` is not 1.0.

The NAM network itself must be run to measure the tones, so the final converter needs NAM inference (port
NeuralAmpModelerCore, MIT, or bind it).

## User IR — SysEx `0x10` (data) + `0x09` (name)

```
→ B1 3B 02, B1 02 1E            app selects CAB page + USER IR 01 (model 30)
→ 70 10 01 <data, sub 0, offset, total 8192>   ×61, each ← 70 10 03 <header> ack
→ 70 09 01 <slot> <packed: 01 00 00 00, 32-byte name "Delay And Reverb">   ← (CC 0x7E 00) + 70 09 03 00…
```

Data header: the same layout as NAM, with sub-command 0. Only slot 0 was captured; the slot is assumed to sit in header
byte 1 as for NAM. The `0x09` record is a 4-byte field (`01 00 00 00` after an upload, meaning unknown) and a 32-byte
name. Note: in the capture this upload sits *just before* the `IR import 'delay and reverb'` marker (the "just
before" markers were typed after the action).

### What the 2048 floats are

* The app converts any audio file to a 48 kHz WAV first (JUCE in `nux_plugins`: `*_ir_48.wav`).
* The uploaded 2048 samples (≈ 42.7 ms at 48 kHz) are **not** a straight copy of the start of the WAV. The test file
  was a long (10 s / 20 s) delay/reverb "IR", and correlation with either candidate file was weak (≈ −0.3 at best,
  with lead-in trimmed so the peak lands at sample ~253). The app likely trims, normalises and possibly windows or
  minimum-phases the IR.
* Re-test with a short, plain cab IR (< 2048 samples) to pin the transform down. Probably just
  trim + normalise + truncate.

Vector: `captures/files/ir/IR01.device_floats.bin` (log marker `imported Jcm800`, cmd 0x10, sub 0). Which of the two
WAVs was used was not recorded (open question).
