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
| 52–55 | `f32` **factor**: 1.0 in every import (the converter's `factor` output, a constant). Not an output gain: an upload with factor 2.0 plays at exactly the same level (re-amped, 2026-10) |
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

Re-amping (a test signal through the pedal and through the real model, see `tools/pedalsim`)
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

### Optimised conversion

The official algorithm designs G1 and G2 from two measurements of the model and never checks what the pedal does with
them. `Measure::Optimised` (`nam::optimise`) simulates the pedal instead and adjusts the filters until the simulated
output sounds like the model:

* **Pedal model.** `y = G2 ⊛ f(G1 ⊛ x)` with 512-tap filters and a fixed piecewise-linear saturation `f` (knots at
  0, ±¼ … ±32 in octaves, flat beyond; fitted jointly to re-amp recordings of three different uploads). It nulls
  against the recordings to the rig's own repeatability (above).
* **Target.** The model's output for the same signal: input −12 dB, the app's DC blocker, × 0.0932 (the pedal's
  output level relative to the model's, measured on the re-amp rig).
* **Test signal** (53 s at 48 kHz). Log sweeps 20 Hz – 20 kHz at −36/−24/−12/−6 dBFS, 1 s tones at 41.2, 55, 73.4,
  98 and 196 Hz at −24/−12/−6 dBFS, and pink noise at about −36, −24 and −12 dBFS. A pink-noise segment at
  −18 dBFS is simulated but not fitted.
* **Filters.** Each is a smooth log-magnitude curve at 41 control points (20 Hz × 2^(i/4), linear in log-frequency)
  made minimum-phase by the folded cepstrum (N = 8192) and cut to 512 taps.
* **Fit.** Multi-resolution STFT log-magnitude L1 distance (windows 4096/1024/256, ¼ hop, 50 ms trimmed from each
  segment end), each bin weighted ∝ 1/f above 40 Hz so every octave counts about the same (`Weighting::Octave`,
  since 0.5; see "Low end" below), plus `1e-3 · mean(max(|u| − 100, 0)²)` to keep `u = G1 ⊛ x` inside the range `f` was fitted on, plus
  `1e-3 ·` the mean squared second difference of each curve. Adam, 300 steps, learning rate 0.05 with a 10-step
  warm-up and cosine decay, starting from the `Measure::Clean` conversion; the best step is kept. Gradients are
  written out by hand and checked against PyTorch's autograd (`tests/data/optim`) and finite differences. About a
  minute per model (optimised build, 8 cores).

Results on the Ampeg SVT "Gain 10 Ultra Lo and Hi MD 421" bass model, scored against the real model (rms dB error of
1/3-octave sweep responses, tone fundamentals and 3rd/5th harmonic ratios):

| Upload | Freq. response | Note level | Harmonics | Overall |
|---|---|---|---|---|
| Official app (recorded) | 7.96 | 9.64 | 11.72 | 8.96 |
| `Measure::Clean` (recorded) | 6.51 | 7.69 | 16.14 | 9.26 |
| PyTorch prototype of the optimiser, two runs (recorded) | 5.70–5.74 | 5.62 | 9.93–10.98 | 6.73–7.02 |
| PyTorch prototype (simulated) | 5.93 | 5.83 | 9.30 | 6.70 |
| `Measure::Optimised` (simulated) | 5.79 | 4.47 | 8.57 | 6.31 |
| `Measure::Optimised`, two runs (recorded) | 5.56 | 4.23 | 10.57–11.00 | 6.72–6.85 |

Recorded in the same session as the prototype's second upload (6.86) and the official one (9.32 that day; the rig
drifted 0.01 dB since the first session). The Rust port differs from the prototype only in leaving out the rig's dry-path EQ from the target (≤ 0.4 dB) and in
its pink-noise generator. Still missing on hardware: the overall level is 3–5 dB low and the model's 60–100 Hz
resonance is up to ~10 dB short. 512 taps (10.7 ms) can't resolve it fully, and the harmonics are limited by the
pedal's fixed `f`. (The table above is the 0.3/0.4 conversion, with the flat weighting.)

#### Low end (0.5)

A re-amp of real bass playing (Ampeg SVT-VR Classic "Normal" model plus an IR, 2026-10-22) found the 0.4 conversion
6–11 dB short at 50–80 Hz while matching the model within ~2–3 dB above 200 Hz; the official conversion, through
the same IR and pedal, was closer there. What it showed:

* **The simulator is right at the low end.** It predicts the recorded sweep responses of both the official and
  the 0.4 upload within ~0.5 dB in every band from 32 to 318 Hz, so the gap was the fit's choice.
* **The loss gave the low end away.** STFT bins are evenly spaced in Hz: below 120 Hz is ~0.5% of the 4096-point
  bins and one or two of the shorter windows' bins. `Weighting::Octave` weights each bin ∝ 1/f above 40 Hz (flat
  20–40 Hz, nothing below 20 Hz, mean 1 per resolution); `Weighting::Flat` is the old loss (the PyTorch reference
  tests use it).
* **What 512 taps can do.** Fitting minimum-phase filters straight to the SVT G10 model's linear response (a +20 dB
  resonance at 63–80 Hz above a ~33 dB/octave cut), the best possible 40–160 Hz error is 7.0 dB rms for 384 taps,
  4.1 for 512, 2.3 for 895 (G2 and the cab IR back to back), 2.1 for 1023 (G1 and G2 together), 0 for 2048.
  With octave weighting the fit reaches the 512-tap limit at 63/79 Hz.

Simulated, 300 steps, flat → octave weighting (rms dB; low end = 40–160 Hz sweep error with the mid level
removed; the harmonics score is unreliable on clean models, whose 3rd/5th harmonics sit near the noise floor):

| Model | Freq. response | Note level | Low end |
|---|---|---|---|
| Ampeg SVT-VR Classic (Normal) | 2.49 → 1.21 | 2.12 → 0.60 | 4.92 → 2.07 |
| Ampeg SVT Gain 10 Ultra Lo and Hi MD 421 | 5.79 → 5.56 | 4.47 → 4.74 | 8.15 → 7.00 |
| Ampeg SVT Ultra Lo MD 421 | 3.31 → 2.79 | 1.85 → 1.70 | 4.83 → 3.39 |
| Orange Terror Bass (Clean) | 1.33 → 0.86 | 1.27 → 0.71 | 1.13 → 0.73 |
| Orange Terror Bass (Dirt) | 2.16 → 1.99 | 2.20 → 1.70 | 1.33 → 1.55 |
| Marshall JCM800 (guitar) | 4.00 → 3.71 | 2.73 → 1.08 | 6.07 → 4.79 |

Amp-only models (the cab comes from an IR) have a gentle low end that 512 taps can follow, and gain the most.
Models with the cab captured in them keep a resonance the filters are too short for. Starting G1 with a steep low cut
(so G1 makes the cut and G2 the resonance) gained another ~1 dB at 63–79 Hz on the G10 in the PyTorch prototype; not
adopted.

**On hardware** (2026-10-22): the same 3-minute bass DI re-amped through the pedal in one session, against the model and
the full IR run in software on the pedal's own dry input, loudness-matched (rms dB; the cab is the Beta52 IR):

| Upload | Overall tone, 40 Hz – 10 kHz | Low end, 40–160 Hz | Moment to moment (median) |
|---|---|---|---|
| Official app NAM, 0.4 IR | 9.44 | 3.04 | 6.18 |
| 0.4 NAM and IR | 3.53 | 5.18 | 4.35 |
| 0.5 NAM, 0.4 IR | 2.79 | 3.78 | 3.64 |
| 0.5 NAM and IR (the IR fit below) | **2.14** | 3.30 | **2.97** |

Run in software on the same input, the 0.5 NAM upload alone is within ±0.5 dB of the model from 50 to 126 Hz; the
remaining dip at 56–80 Hz is the IR's 384 taps (even the exact model through the pedal's IR is 7 dB short at 63 Hz).
Fitting G2 and the IR together (895 taps) would reach further, but would tie the AMP slot to one cab.
`tools/pedalsim/reamp_compare` has the analysis; the docs site shows it interactively.

## User IR — SysEx `0x10` (data) + `0x09` (name)

```
→ B1 02 1E+slot                 app selects the slot's cab model (USER IR 01 = 30); it may show the CAB page first (B1 3B 02)
→ 70 10 01 <data, sub 0, offset, total 8192>   ×61, each ← 70 10 03 <header> ack
→ 70 09 01 <slot> <packed: 01 00 00 00, 32-byte name>   ← (CC 0x7E 00) + 70 09 03 00…
```

Data header: the same layout as NAM, but the slot is in **header byte 0** (byte 1, NAM's slot byte, is 0) and the
sub-command is 0. The `0x09` record is a 4-byte field (`01 00 00 00` in every import seen, eleven of them) and a 32-byte
name: the file name without its extension, **cut to 16 characters**. Checked byte for byte against imports into all ten
slots (`captures/logs/02-ir-tests.log`, `tests/ir_import.rs`).

### What the 2048 floats are (the official converter, 2026-10)

Recovered from the app's native code (`nux_plugins`: `JuceUtil::convertAudioToWaveFormat`, decompiled with Ghidra)
and confirmed on eleven captured imports: ten designed test IRs (`tests/data/ir`, `examples/ir_test_files`) and a
real reverb IR. `ir::convert` reproduces all of them (eight bit for bit, the rest within float rounding,
`examples/ir_capture_check`).

1. **Normalise the file.** Unless it is already a 48 kHz mono 24-bit WAV, it is converted to one: JUCE's
   `ResamplingAudioSource` (`samplesIn/samplesOut = rate / 48000`) writes `⌊len · 48000 / rate⌋` samples of the
   **first channel** to a 24-bit WAV (`*_ir_48.wav`). The resampler is linear interpolation (single precision) plus
   JUCE's 2nd-order low-pass at the lower Nyquist frequency, applied to the input when downsampling and to the output
   when upsampling (not at all within ±0.01 % of 1:1). 24-bit: `round(x · 8388607)`, read back as `/ 8388608`.
2. **Take the first 2048 samples** (zero-padded), **time-reversed**: `data[2047 − k] = g · ir[k]`, the same layout
   as the NAM filters. No trimming, windowing or minimum-phase conversion.
3. **Gain.** The app's `NuxFFT` (16384 points) transforms the 2048 samples (as the impulse response of the FIR they
   form) and scans bins 136–1023, **398 Hz – 3 kHz**, for the smallest and largest magnitude:
   `g = 1 / √(max |X| · min |X|)`, so the band's peak and trough end up symmetric about 0 dB. A single impulse
   becomes exactly 1.0.

`convertAudioToWavWithOut` writes 2048 floats back out reversed as a 24-bit WAV (an export, not part of the import).

### What the pedal plays: 384 of the 2048 IR samples (2026-10)

Re-amping probe IRs (impulses at known taps, uploaded to USER IR 10, the CAB block on its own, sweep deconvolution
divided by the dry path; `tools/pedalsim/ir_reamp_*.py`) shows that the pedal plays only the **first 384 taps (8 ms)**
of an uploaded IR: impulses at taps 0–383 come back at their uploaded amplitude (within a few %), taps 384, 450, 500,
1000, 1500 and 2040 not at all. In upload order that is `data[1664..2048]`; `data[0..1664]` is ignored. The CAB block's
level is about unity (an impulse of 1.0 at tap 0 measures −0.1 … −0.8 dB against the dry path).

So the official conversion's last 1664 samples are never heard, and its gain is computed on a response the pedal
doesn't play. Like the NAM filters' 512-tap limit, this leaves room for a better conversion of long IRs.

### Import level

The official conversion plays models at their own captured level scaled down a lot: on the pedal, 24–34 dB below the
model's own output for the same input, so imports sit well below the built-in amps (13–19 dB for the SVT-VR models
tested) and need the AMP block's gain and master turned up. NAM plugins instead normalise every model to one level.
`nam::convert::normalise_level` does the same: it measures what the pedal would play for the model (pink noise at
−18 dBFS, the optimiser's calibration) and scales G2 to `NORMALISED_LEVEL_DB`. G2 comes after the pedal's saturation,
so this changes only the volume. Calibrated on the pedal: normalised imports play at the level of the built-in Ampug
SV Bass and Mes 400 Bass with the same knobs, and two models 6 dB apart raw land within 0.2 dB of each other.

### Optimised IR conversion

`ir::convert_optimised` (`Method::Optimised`) designs the 384 taps the pedal plays from the whole IR:

1. Resample with a windowed sinc (`ir::resample`) instead of JUCE's linear interpolation.
2. Target: the full IR's magnitude response, 1/6-octave smoothed (384 taps can't resolve finer detail).
3. A minimum-phase filter with that response (folded cepstrum, 32768 points): no leading delay, energy packed into
   the start. A few iterations correct the design for what the 384 taps (faded over the last 48) actually produce.
4. Then the taps themselves are fitted (Adam, 400 steps, hand-written gradient through the FFT) to the full IR's
   levels in 1/6-octave bands every 1/12 octave from 30 Hz to 16 kHz, level left free. (Until 0.4 the fit ran
   25 Hz – 20 kHz; a cab IR has next to nothing above 16 kHz, and matching that cost taps the rest needs.)
5. Level: what the app means to give the IR, its own gain rule on the first 2048 samples counted from the IR's onset,
   matched over 100 Hz – 8 kHz. The unused 1664 samples of the upload are zero.

About half a second per IR. In software, on fifteen Ampeg bass IRs (`examples/ir_compare`: 1/3-octave error of what
the pedal plays against the full IR, 31.5 Hz – 16 kHz, level difference removed):

| IRs | Official conversion | Optimised |
|---|---|---|
| 8 × "8x10" cab IRs (mic'd, start at sample 0) | 2.5 – 5.4 dB rms, worst band +7 … +23 dB (sub-bass) | 0.9 – 3.2 dB rms (0.4: 1.0 – 3.5) |
| 5 × "SVT" IRs that start after 384 samples | 12.7 – 14.2 dB rms, **38–44 dB too quiet** (the pedal plays the silence before the IR) | 2.3 – 2.4 dB rms (0.4: 2.7 – 2.9), level within 0.4 dB |
| 2 × DI recordings that start after 2048 samples | 9.9 – 13.6 dB rms, 34 dB too quiet | 2.9 – 3.6 dB rms (0.4: 2.8 – 3.3); "D-I-Out Hot" plays 16 dB quiet (the level rule; unchanged) |
| Mean | 7.94 dB | 2.18 dB (0.4: 2.42) |

**On hardware** (the 0.4 conversion; re-amped, the CAB block alone, measured response against the full IR, same measure): "Ampeg 8x10 57
AH" official 4.78 dB rms, optimised **2.07 dB** at the same level (+0.3 dB); "Ampeg SVT Neumann" (starts 1030 samples
in) official 16.0 dB rms and **46 dB too quiet**, optimised **5.47 dB** at the intended level. The recorded responses
match the uploaded 384 taps (correlation 0.97–0.99, within about 1.5 dB at 32 Hz and 0.6 dB above). The extra error
against software is mostly the 32 Hz band, which a 384-sample window resolves poorly. `Device::import_ir_file` uses
the optimised conversion by default.

The first capture (`captures/files/ir/IR01.device_floats.bin`, log marker `imported Jcm800`) was the "percussive verb 1"
WAV: gain 12.5344, reproduced to −138 dB.
