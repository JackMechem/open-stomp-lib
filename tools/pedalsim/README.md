# pedalsim: pedal simulator and NAM filter optimiser (research)

The analysis behind `nam::optimise` / `Measure::Optimised` (docs/05, "Optimised conversion"). This is the Python and
PyTorch prototype; the Rust port in `src/nam/optimise.rs` is tested against it (`golden.py` →
`tests/data/optim`).

Findings:
- The pedal runs only the first **512** impulse-response taps of each uploaded 1024-tap filter.
- A user AMP behaves as `y = G2 ⊛ f(G1 ⊛ x)`, with `f` a fixed saturation curve. `f` is fitted jointly from
  re-amp recordings of three uploads, and the fit nulls them at −23…−24 dB (the rig repeats at −24 dB).
- Optimising G1/G2 against the real NAM through this simulator beats the official conversion on hardware
  (Ampeg SVT "Gain 10 Ultra Lo and Hi MD 421"):

| Upload (re-amped) | Overall rms feature error vs the real model |
|---|---|
| Official app | 8.96–9.32 dB |
| Clean conversion | 9.06–9.26 dB |
| This prototype (PyTorch) | 6.73–7.02 dB |
| The Rust port (`Measure::Optimised`) | 6.72–6.85 dB |

| Script | Purpose |
|---|---|
| `paths.py` | where the data lives (see below) |
| `audio_io.py`, `reamp_run.py` | play the test signal (M-Track out → pedal in) and record the pedal's USB audio, per AMP slot |
| `reamp_analyze.py`, `reamp_plots.py` | sweeps, harmonics and null tests between runs |
| `nam_features.py`, `nam_plots.py`, `plots3.py`, `predict.py` | the real NAM in software vs the recordings; input/output calibration |
| `pedalsim.py` | the simulator; fits `f` from recordings (`pedalsim_f.npy`) |
| `optimize.py` | target render, differentiable simulator, spectral loss (free-tap optimiser) |
| `optimize2.py` | optimiser over smooth minimum-phase curves (the one ported to Rust) |
| `eval_feats.py`, `score_opt.py` | feature-metric score of simulated / recorded runs vs the real NAM |
| `golden.py` | reference values for the Rust port's tests |
| `readme_plots.py` | the frequency-response graphs in `docs/images` (`--cached` uses `results/`) |

## Data

Not in git: the recordings (~200 MB), the official app's converter dumps (vendor output) and the third-party model.
`paths.py` reads:

- `PEDALSIM_DATA`: directory with `reamp/` (`svt_g10`: recordings and fits; `svt_g10_opt`, `svt_g10_rust`: the
  optimiser's hardware runs) and `namconv/` (dumps from open-stomp-manager `tools/namconv`). Default: a sibling
  open-stomp-manager checkout's `analysis/extracted/`.
- `PEDALSIM_MODEL`: the `.nam` file.

`results/svt_g10_responses.json` holds the measured 1/3-octave responses behind those graphs, so those can be
redrawn without the recordings.

## Setup

    python3.12 -m venv .venv && .venv/bin/pip install -r requirements.txt ../../bindings/python

Examples:

    .venv/bin/python optimize2.py C 300 out.npy
    .venv/bin/python eval_feats.py out.npy
    .venv/bin/python reamp_run.py . <out dir> '[["A1",35],["R1",39],["DRY",null]]'
    .venv/bin/python score_opt.py <out dir> A1,R1
