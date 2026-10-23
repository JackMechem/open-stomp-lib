# Fixing the low end

*October 2026, open-stomp 0.5.*

## The problem

A bass DI re-amped through the pedal showed open-stomp 0.4's NAM conversion matching the model within 2–3 dB
above 200 Hz, but **6–11 dB short at 50–80 Hz**. The model was the Ampeg SVT-VR Classic (Normal) with the
Ampeg SVT Beta52 cab IR. The official app's conversion, much further off everywhere else, was closer down there.
For a bass amp that's where it matters most.

## Finding the cause

There were three suspects: the pedal simulator, the filter length, and the fit itself.

* **Not the simulator.** It predicted the recorded sweep responses of both uploads within about 0.5 dB in every
  band from 32 to 318 Hz. The optimiser *knew* it was leaving the low end short.
* **Not mainly the filter length.** Fitting 512-tap minimum-phase filters straight to the model's response, the
  best possible 40–160 Hz error is far below what 0.4 achieved. The limit is real (see below), but 0.4 wasn't
  near it.
* **The loss.** The optimiser compares spectra with an STFT, whose bins are evenly spaced in Hz. Below 120 Hz is
  about **0.5% of the bins** in the 4096-point window, and only one or two bins in the shorter ones. Trading 8 dB at
  63 Hz for a fraction of a dB in the mids looked like a good deal to the fit.

## The fix

Weight each bin by 1/f, so every **octave** counts about the same (flat from 20–40 Hz, nothing below 20 Hz,
normalised to a mean of 1). It's `Weighting::Octave` in `nam::optimise`, now the default. `Weighting::Flat`
keeps the old loss.

In simulation, across six models (rms dB, flat → octave weighting):

| Model | Frequency response | Note level | Low end (40–160 Hz) |
|---|---|---|---|
| Ampeg SVT-VR Classic (Normal) | 2.49 → **1.21** | 2.12 → **0.60** | 4.92 → **2.07** |
| Ampeg SVT Gain 10 Ultra Lo and Hi MD 421 | 5.79 → **5.56** | 4.47 → 4.74 | 8.15 → **7.00** |
| Ampeg SVT Ultra Lo MD 421 | 3.31 → **2.79** | 1.85 → **1.70** | 4.83 → **3.39** |
| Orange Terror Bass (Clean) | 1.33 → **0.86** | 1.27 → **0.71** | 1.13 → **0.73** |
| Orange Terror Bass (Dirt) | 2.16 → **1.99** | 2.20 → **1.70** | 1.33 → 1.55 |
| Marshall JCM800 (guitar) | 4.00 → **3.71** | 2.73 → **1.08** | 6.07 → **4.79** |

The frequency response improved on all six and the low end on five. The two scores that got worse moved by 0.3 dB
or less.

The IR conversion got a smaller fix: its fit now covers **30 Hz–16 kHz** instead of 25 Hz–20 kHz. Cab IRs have
next to nothing above 16 kHz, and matching that wasted taps the rest of the range needed. On fifteen Ampeg IRs
the error of what the pedal plays dropped from 2.42 to 2.18 dB rms, and at 40–160 Hz from 3.55 to 2.90 dB.

## On the pedal

The same DI, re-amped in one session through four versions. The reference is the original model and the full
IR run in software on the pedal's own dry input:

![Difference from the original model and IR by frequency for each version](images/lowend-difference.png)

| | Official app NAM, 0.4 IR | 0.4 NAM + IR | 0.5 NAM, 0.4 IR | **0.5 NAM + IR** |
|---|---|---|---|---|
| Overall tone (40 Hz–10 kHz) | 9.4 dB | 3.5 dB | 2.8 dB | **2.1 dB** |
| Low end (40–160 Hz) | 3.0 dB | 5.2 dB | 3.8 dB | **3.3 dB** |
| Moment to moment (median) | 6.2 dB | 4.3 dB | 3.6 dB | **3.0 dB** |
| Closest to the original in | 8% | 2% | 3% | **87%** of 100 ms windows |

The new NAM conversion and the new IR conversion each help, and together 0.5 is the closest version in almost
every moment of the recording. The official app still has a slightly lower low-end score, but it's 7–17 dB
dark above 400 Hz.

**[Open the interactive comparison](comparison.md)** for the full spectra, the error over time, a
time × frequency heatmap, dynamics and per-band tables.

## What's left: the cab

Run in software on the same input, the 0.5 NAM upload alone is within **±0.5 dB of the model from 50 to 126 Hz**.
So the remaining dip at 56–80 Hz in the graph is the cab, not the amp.

The Beta52 IR has a resonance just above a steep low cut, and the pedal plays only 384 taps (8 ms) of it. Even a
*perfect* NAM conversion through that IR is 7 dB short at 63 Hz. The best possible 40–160 Hz error for a
minimum-phase filter fitted to this kind of response:

| Taps | Best 40–160 Hz error |
|---|---|
| 384 (the pedal's IR) | 7.0 dB |
| 512 (one NAM filter) | 4.1 dB |
| 895 (the NAM's G2 and the IR back to back) | 2.3 dB |
| 1023 (both NAM filters) | 2.1 dB |
| 2048 | 0 dB |

Fitting the NAM's output filter and the IR together would get close to the 895-tap row. The catch is that the
AMP slot would then only sound right with that one cab. It isn't implemented.
