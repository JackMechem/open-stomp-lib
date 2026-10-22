# 05 — User AMP (NAM) and User IR uploads

Status: **user AMP (NAM) import works end to end**: the transport (`open_stomp::upload`, rebuilt byte for byte from every
captured import), NAM inference (`open_stomp::nam`, a port of NeuralAmpModelerCore) and the app's converter
(`open_stomp::nam::convert`, [below](#our-implementation)). **User IR import**: transport done, the audio → 2048 floats
conversion is not reversed yet.

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

### The algorithm (`NamFileThread::run`)

Recovered from the disassembly (Ghidra) and **verified stage by stage**: given the model's input/output for each call
(captured by the harness with `--log-io`), the open-stomp-manager `tools/namconv/reference.py` reproduces every
vendor buffer for both test files (tone levels to ≤ 1e-5, `g1GainsDb` to ≤ 0.011 dB, G1IR/G2IR/upload to ≤ 7e-5
relative; the rest is float32 vs float64 FFT rounding). The model is a **Wiener–Hammerstein** approximation:
*G1 → the pedal's own static nonlinearity → G2*, with G1·G2 = the network's small-signal response.

Building blocks:

* **Spectrum**: an 8192-point FFT (TI DSPLIB `DSPF_sp_fftSPxSP`, no window, no scaling) of real input; bins
  0…4095, as power `re² + im²` or magnitude.
* **DC blocker** on every model output: biquad `b = [0.9953827, −1.990765, 0.9953827]`,
  `a = [1, −1.990744, 0.9907867]`, float32. Its state is reset to zero once, before the BLA run, and then **carries
  over** from call to call (BLA, then each tone in order). The NAM model's own state carries over the same way.
* All model calls run at 48 kHz on one channel, in this order: the BLA run (8192 samples), then 94 tones
  (10240 samples each).

Steps:

1. **Best linear approximation.** `y = model(BLA_x)`, DC-blocked. `BLA_x` is an 8192-sample constant in the binary:
   a pink random-phase multisine over every bin, amplitude `√(1/max(f, 40 Hz))` scaled by 1e-5 (peak ≈ 3e-5, about
   −90 dBFS), so the model is driven in its small-signal range. Its phases don't come from `rand()`; it was
   apparently generated offline. `H[k] = |Y[k]| / |X[k]|` for k = 0…4095, and `H_dB = 20·log10 H`.
2. **Smoothing** (1/24 octave): for k = 2…4095, `lo = max(floor(k / 2^(1/48)), 2)`,
   `hi = clamp(ceil(k · 2^(1/48)), lo, 4095)` (`fcvtms` / `fcvtps`), `m = max(k, lo)`. Weights are triangular in
   log-frequency: `w_j = (log j − log(lo−1)) / (log m − log(lo−1))` for j ≤ m, `(log(hi+1) − log j) / (log(hi+1) − log m)`
   for j > m. `Hs[k] = sqrt(Σ w_j H_j² / Σ w_j)`, and `Hs[0] = Hs[1] = Hs[2]`.
3. **Tone test.** For each of the 94 bins b in `ToneTest_FftBins` (58.6 Hz … 14.6 kHz, ~1/12 octave): input
   `sinf(n · b · 2π/8192)` for n = 0…10239 (amplitude 1.0), model, DC blocker, power spectrum of samples
   2048…10239. `level = √P[b] · (π/4) / 4096`; for the first 83 tones also `level3 = √P[3b] · (3π/4) / 4096`
   (the 3rd harmonic).
4. **Gains.** `F[k]` = levels linearly interpolated between tone bins (held at the first/last value outside),
   `T[k]` = 3rd-harmonic levels interpolated over the `3b` bins (0 below, last value above), `M = max(F, T)`.
   **`g1GainsDb[i] = 20·log10(Hs[b_i] / M[b_i])`.**
5. **Curves.** `G1_dB[k]` = `g1GainsDb` interpolated **in log-frequency** between tone bins (held outside).
   `G2_dB[k] = H_dB[k] − G1_dB[k]` (unsmoothed H).
6. **Filters.** Each curve → linear magnitude, mirrored to 8192 points as `m[8191−k] = m[k]` (note: not `m[N−k]`),
   then `Magnitude2MinPhase`: `φ = −imag(hilbert(ln m))` (`ln 0 → −30`), `IR = real(ifft(m · e^{jφ}))` (1/N-scaled).
   This gives `G1IR` and `G2IR`.
7. **Upload.** `g1 = reverse(G1IR[0:1024])`, `g2 = reverse(G2IR[0:1024])`, **`factor` = 1.0 (a constant in the
   code)**.

The rest of `run()` (FIR objects, "match EQ" buffers, the `srand(33)` odd-prime multisine `levelsTesterItem`) sets
up other features and does not affect the upload.

Two quirks are part of what the pedal gets, and the open converter copies both:

* **The DC transient.** The model's output has a DC offset (≈ 3e-4 for both test files, 10× the BLA signal) and the
  DC blocker starts from zero, so its few-hundred-sample transient leaks into the BLA spectrum. The app's `H` is far
  from the model's true small-signal response where the transient dominates: about 30–38 dB too high below 100 Hz,
  5–30 dB above 4 kHz, a few dB in between. G1 and G2 inherit this.
* **The mirror.** `m[8191−k] = m[k]` (instead of `m[N−k]`) plus taking the real part blends neighbouring bins, so
  `G1IR`/`G2IR` miss jagged target curves by up to ~1–3 dB per bin (`G1IR` vs `g1GainsDb`: mean 0.01–0.13 dB, std
  0.5–1.1 dB in the app's own output).

### What the pedal runs: 512 taps per filter (2026-10)

Re-amping (a test signal through the pedal and through the real model, see open-stomp-manager `tools/pedalsim`)
shows that the pedal runs only the **first 512 taps** of each uploaded impulse response: `data[512..1024]` for G1 and
`data[1536..2048]` for G2 (taps 511..0, reversed). `data[0..512]` and `data[1024..1536]` are ignored. A model of the
pedal as

    y = G2_512 ⊛ f(G1_512 ⊛ x)

with `f` a fixed piecewise-linear saturation (the same for every user AMP) nulls against the recorded output of the
official, open and clean uploads to −23.4 … −24.4 dB, the rig's own repeatability (−24.4 dB between two recordings of
the same upload). With all 1024 taps it can't: the clean conversion's 41 Hz response is then off by ~10 dB. The
truncation is also why every 1024-tap conversion so far (the official one included) has excess sub-bass on the pedal:
the cut-off tail of a minimum-phase response carries part of its low end.

`convert::PEDAL_TAPS`, `convert::upload_data` (two responses → upload, unused halves zero) and `convert::pedal_irs`
(upload → the responses the pedal runs). `Measure::App` keeps the app's 1024-tap layout byte for byte; the other modes
upload exactly what the pedal runs.

### Our implementation

`open_stomp::nam` (feature `nam`):

* **Inference.** `nam::Model` runs WaveNet models: plain (NAM 0.5 "classic", e.g. the standard A1 architecture) or in a
  `SlimmableContainer` (0.7, the full-size submodel, as NeuralAmpModelerCore defaults to). Gated and ungated layers,
  bottleneck, `layer1x1`/`head1x1`, kernel heads. Not supported: LSTM/ConvNet models, FiLM, grouped convolutions,
  condition DSPs, post-stack heads. It matches NeuralAmpModelerCore to ≤ 3e-5 on its example models
  (`tests/data/nam/`), and the vendor's own model output to ~1e-6 relative over a whole conversion.
* **Conversion.** `nam::convert(&mut model)` → `Converted { data: [f32; 2048], factor: 1.0, … }`, about 4 s per model
  (optimised build). `Device::import_nam_file(slot, path)` converts and uploads.

Accuracy against the official app (the two test files; 1/3-octave magnitude response of the uploaded filters):

| Excitation | g1 | g2 |
|---|---|---|
| The app's `BLA_x` (`convert_with` + `official_excitation`, read from your own install) | ≤ 0.01 dB | ≤ 0.03 dB |
| Ours (`excitation()`, the default) | 0.7–1.6 dB rms (max 2–7) | 1.0–1.3 dB rms (max 2–4) |

**On hardware** (Dual Stomp Lite, V1.1.1): "Ampeg SVT – MD 421" (a `SlimmableContainer` bass model) converted by the
official converter (run headless) and by ours, both uploaded with `Device::import_user_amp`, sound identical; on paper
they differ by ≤ 0.15 dB (1/3-octave). The Jcm800 and FNDR models differ more on paper (below), because their DC offset
is larger.

The difference with our excitation is the app's own measurement noise: where the DC transient and the signal
interfere (roughly 100 Hz – 1 kHz), the result depends on the excitation's phases, so the app itself would give a
different answer by this much with any other table. Averaging over phases (computable in closed form) was tried and
is not closer to the app's single measurement, and neither is the app's own spectrum with other phases (tested: error
energy −0.2 to −31 dB vs the app's upload, the same spread as ours), so only the app's exact table reproduces its
results. `BLA_x` can't be regenerated (it isn't `rand()`-derived) and is vendor data, so it isn't shipped:
`nam::convert::official_excitation(path)` reads it from the user's own copy of Stomp Manager (the `.app` bundle,
the Windows install folder or the library file; nothing is executed, the table is found by an 8-byte prefix and a
64-bit hash), and `find_official_excitation()` tries the standard install locations. This is strictly opt-in:
nothing in open-stomp reads the app by itself (`Device::import_nam_file` always uses our own converter). With it,
conversions match the official app exactly (verified on ten models: error energy −51 to −93 dB vs the app's
uploads). It works on any OS with a downloaded copy of
the app; the Windows build's table is assumed identical (not checked).

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
