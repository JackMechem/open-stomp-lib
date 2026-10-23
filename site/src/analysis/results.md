# Results on hardware

Every number here was measured on the pedal: the conversions were uploaded, a signal was played into the
pedal's input, and its USB audio output was recorded. That recording was compared with the original model
or IR run in software. Lower is better everywhere.

## NAM: sweep measurements

The Ampeg SVT "Gain 10 Ultra Lo and Hi MD 421" model, converted three ways and re-amped in one session: sweeps at
four input levels and bass tones. Each upload is compared with the model run in software on the pedal's own dry take
of the same signal. Scores are rms dB, with levels matched:

| Upload | Frequency response | Low end (40–160 Hz) | Note dynamics | Harmonics |
|---|---|---|---|---|
| Official app | 4.75 dB | 4.95 dB | 9.34 dB | 12.16 dB |
| open-stomp 0.4 | 3.05 dB | 5.35 dB | 5.84 dB | 10.85 dB |
| open-stomp 0.5 | **2.14 dB** | **3.51 dB** | **2.88 dB** | **8.60 dB** |

*Note dynamics* is how the level of each note changes with how hard it's played (fundamental levels of bass tones at
three levels, after removing one overall offset). *Harmonics* compares the 3rd and 5th harmonics relative to each
note's fundamental.

![NAM frequency response on the pedal: the real model and the three conversions](../lib/images/nam-frequency-response.png)

![NAM: difference from the real model for the three conversions](../lib/images/nam-frequency-error.png)

The graphs compare tone, so each upload is level-matched to the model over 100 Hz–8 kHz. The official conversion is
off by up to 10–13 dB in the 400 Hz–1 kHz mids at low input levels, 5–8 dB in the upper mids and up to 20 dB in the
top octave. Both open-stomp versions stay within 4 dB from 100 Hz to 10 kHz. Below that, this model has a +20 dB
resonance at 63–80 Hz that 512 taps can't fully follow: 0.5 gets within 4–8 dB of it, 0.4 within 7.5–10.

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

The CAB block on its own, re-amped in the same session and measured by sweep, against the full IR file in software
(31.5 Hz–16 kHz, levels matched):

![IR frequency response on the pedal: the full IR file and the three uploads](../lib/images/ir-frequency-response.png)

![IR: difference from the full IR file for the three uploads](../lib/images/ir-frequency-error.png)

| IR | Official app | open-stomp 0.4 | open-stomp 0.5 |
|---|---|---|---|
| Ampeg 8x10 57 AH | 5.28 dB rms | **1.36 dB rms** | 2.11 dB rms |
| Ampeg SVT Neumann | 16.13 dB rms, and 45 dB quieter | **5.13 dB rms** | 5.23 dB rms |

The SVT Neumann file starts with 21 ms of silence, so the official upload plays almost none of the cab.

In 0.5 the IR fit covers 30 Hz–16 kHz instead of 25 Hz–20 kHz. In software, on fifteen Ampeg cab IRs, that lowered
the average error of what the pedal plays from 2.42 to 2.18 dB rms. On the pedal the picture is mixed: on the
Neumann it is up to 1.8 dB closer at 50–100 Hz, but both IRs lose 1–4 dB in the 32–40 Hz bands, below a bass's low E,
and that's where these two scores get worse. In the real-playing test above, the 0.5 IR conversion of the Beta52 cab
improved the overall tone from 2.8 to 2.1 dB.

## Reproducing these

The scripts are in [`tools/pedalsim`](https://github.com/JackMechem/open-stomp-lib/tree/main/tools/pedalsim):
`readme_plots.py <session>` measures a re-amp session (the `reamp_run.py` test signal, one block at a time) and draws
the graphs above; `readme_plots.py --cached` redraws them from `tools/pedalsim/results`. The real-playing comparison is in
[`tools/pedalsim/reamp_compare`](https://github.com/JackMechem/open-stomp-lib/tree/main/tools/pedalsim/reamp_compare).
