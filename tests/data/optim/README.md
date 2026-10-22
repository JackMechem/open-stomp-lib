# Optimiser golden data

Reference values for `nam::optimise` (the unit tests in `src/nam/optimise.rs`), produced by the Python/PyTorch
optimiser that made the hardware-validated upload: `tools/pedalsim/golden.py`
(`golden.py <this dir>`). All inputs are synthetic or come from the repository's own MIT test model
(`../nam/wavenet.nam`); no third-party or vendor data.

Each `<name>.bin` is little-endian `f64`; `golden.json` lists the lengths and the scalar results.

| Files | What |
|---|---|
| `f_*` | the pedal's saturation curve `f` and its derivative at 1018 points |
| `co_*` | `curve_of`: 512-tap responses (a clean conversion of the test model) → 41 control-point curves |
| `mp_*` | curve → minimum-phase 512 taps, and the gradient of `Σ gh·h` w.r.t. the curve |
| `sim_*` | the pedal simulator on a 20000-sample signal: `u`, `y`, the gradients of `Σ gy·y + penalty(u)` w.r.t. both filters |
| `loss`, `loss_gy` | the multi-resolution STFT loss of `sim_y` against `sim_tgt` and its gradient |
| `opt_*` | eight steps of the optimiser loop (Adam, schedule, penalties) from `opt_c0` |
| `tgt_*` | the target render: test model → DC blocker → gain |
| `sig_*` | 4000 samples of the re-amp test signal (all parts but the pink noise) |
