# How the pedal plays NAM and IRs

The Dual Stomp Lite can load a [Neural Amp Modeler](https://www.neuralampmodeler.com/) (`.nam`) amp model into
a user AMP slot and a cab impulse response (`.wav`) into a user IR slot. But the pedal can't run either one as
it is. What it actually runs is much simpler, so every import is an approximation. How good that approximation
is depends on the converter.

## What the pedal runs instead of a NAM model

A NAM model is a neural network (a WaveNet). The pedal runs a fixed structure instead:

```text
input ──► G1 ──► f ──► G2 ──► output
          512-tap     fixed     512-tap
          filter    saturation  filter
```

Two FIR filters with a fixed saturation curve between them, a *Wiener–Hammerstein* model. An import is just
the two filters: 2048 floats, of which the pedal plays 512 per filter. That was found by re-amping uploads
with test patterns in them ([docs/05](../lib/05-uploads-nam-ir.md#what-the-pedal-runs-512-taps-per-filter-2026-10)).

**The official app's conversion** measures the model twice: a small-signal frequency response, and a sine tone
test (fundamental and third harmonic). It turns each measurement into a minimum-phase filter. It's fast, but it
doesn't check whether the result *sounds* like the model on the pedal.

**open-stomp's optimised conversion** starts from the same measurements, then fits the two filters to the
model:

1. **Simulate the pedal.** `y = G2 ⊛ f(G1 ⊛ x)`, with the saturation curve `f` fitted from re-amp recordings.
   The simulator predicts the hardware within about 0.5 dB.
2. **Compare against the model** on sweeps, tones and pink noise at several levels, with a
   multi-resolution spectral loss.
3. **Adjust the filters** (hand-written gradients, Adam, about a minute) until the simulated pedal matches the model
   as closely as 512 taps allow. Both filters stay smooth and minimum-phase.

Since version 0.5 the loss weights each octave about the same. Before that the low end counted for almost
nothing, which [Fixing the low end](low-end.md) explains.

## What the pedal plays of an IR

A user IR upload is 2048 samples, but **the pedal plays only the first 384 (8 ms at 48 kHz)**. Found the same
way: impulses at taps 0–383 come back, later ones don't.

**The official app's conversion** takes the start of the file as it is. The pedal then cuts it off at 8 ms.
Many cab IRs start after a short delay or ring for longer than that. Those lose most of their sound, and an IR
that starts after the cutoff plays as near silence (35–45 dB too quiet).

**open-stomp's optimised conversion** designs a 384-tap minimum-phase filter whose response matches the *whole*
IR as closely as possible. It packs the cab's tone into the part the pedal plays, then sets the level the app
meant to give the IR. It takes under a second.

## Limits

Some differences no converter can remove:

* **Phase.** Both conversions are minimum-phase, so they match how the model *sounds* (its magnitude response),
  not its waveform.
* **Filter length.** A cab with a deep, narrow low-end resonance needs longer filters than 384 or 512 taps.
  The remaining gap at 60–80 Hz on bass cabs comes from this.
* **Nonlinearity.** The saturation curve is fixed. High-gain models whose distortion changes character with
  level or frequency can only be approximated.

## Where it's documented

* [NAM and IR uploads](../lib/05-uploads-nam-ir.md): the upload protocol, the official algorithms as
  recovered from the app, and the optimised conversions in detail.
* [Results on hardware](results.md): the measurements.
* The analysis code: [`tools/pedalsim`](https://github.com/JackMechem/open-stomp-lib/tree/main/tools/pedalsim)
  (Python: re-amp rig, pedal simulator, PyTorch prototype of the optimiser, scoring).
