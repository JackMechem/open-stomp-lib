# Results on hardware

Every number here was measured on the pedal: the conversions were uploaded, a signal was played into the
pedal's input, and its USB audio output was recorded. That recording was compared with the original model
or IR run in software. Lower is better everywhere.

## NAM: sweep measurements

Ampeg SVT bass model, sweeps and tones at four input levels. The scores are rms error over 1/3-octave sweep
responses, note levels and harmonics, with levels as uploaded before normalisation. Both conversions here are from
open-stomp 0.4, before the [low-end fix](low-end.md).

| Upload | Frequency response | Note level | Harmonics | Overall |
|---|---|---|---|---|
| Official app | 7.96 dB | 9.64 dB | 13.11 dB | **9.32 dB** |
| open-stomp 0.4, optimised (two runs) | 5.56 dB | 4.23 dB | 10.57–11.00 dB | **6.72–6.85 dB** |

![NAM frequency response on the pedal: the real model, the official app's conversion and open-stomp's](../lib/images/nam-frequency-response.png)

![NAM: difference from the real model for both conversions](../lib/images/nam-frequency-error.png)

The graphs compare tone, so each upload is level-matched to the model over 100 Hz–8 kHz. With levels matched, the
official conversion is off by up to 15–24 dB in the deep bass, in the 400 Hz–1 kHz mids at low input levels, and
in the top octave. open-stomp stays within about 5 dB from 50 Hz to 10 kHz.

## NAM: real playing

A 3-minute bass DI re-amped through the pedal with the Ampeg SVT-VR Classic (Normal) model and a cab IR, against
the model and IR in software on exactly what the pedal heard. [Fixing the low end](low-end.md) has the details,
and the [interactive comparison](comparison.md) has every graph.

| | Official app | open-stomp 0.4 | open-stomp 0.5 |
|---|---|---|---|
| Overall tone (40 Hz–10 kHz) | 9.4 dB | 3.5 dB | **2.1 dB** |
| Low end (40–160 Hz) | 3.0 dB | 5.2 dB | **3.3 dB** |
| Moment to moment (median) | 6.2 dB | 4.3 dB | **3.0 dB** |
| Closest to the original in | 8% | 2% | **87%** of 100 ms windows |

## IR: cab measurements

The CAB block on its own, measured by sweep, against the full IR file in software, levels matched:

![IR frequency response on the pedal: the full IR file, the official app's upload and open-stomp's](../lib/images/ir-frequency-response.png)

![IR: difference from the full IR file for both uploads](../lib/images/ir-frequency-error.png)

| IR | Official app | open-stomp, optimised |
|---|---|---|
| Ampeg 8x10 57 AH | 3.95 dB rms | **2.36 dB rms** |
| Ampeg SVT Neumann | 14.97 dB rms, and about 40 dB quieter | **5.87 dB rms** |

The SVT Neumann file starts with 21 ms of silence, so the official upload plays almost none of the cab.

In software, on fifteen Ampeg cab IRs, what the pedal plays is on average 7.9 dB rms from the full IR with the
official conversion, 2.4 dB with open-stomp 0.4, and 2.2 dB with 0.5. In 0.5 the fit was limited to
30 Hz–16 kHz: cab IRs have almost nothing above that, and matching it cost taps the rest of the range needed.

## Reproducing these

The scripts are in [`tools/pedalsim`](https://github.com/JackMechem/open-stomp-lib/tree/main/tools/pedalsim):
`reamp_run.py` drives the re-amp rig, `reamp_analyze.py` and `ir_reamp_analyze.py` score the recordings,
`readme_plots.py --cached` redraws the graphs above. The real-playing comparison is in
[`tools/pedalsim/reamp_compare`](https://github.com/JackMechem/open-stomp-lib/tree/main/tools/pedalsim/reamp_compare).
