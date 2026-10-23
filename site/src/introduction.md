# Open Stomp

Open tools for the **SORAN Dual Stomp Lite** guitar multi-effects pedal (USB `3962:2000`, "JME STOMPBOX"):

* **[Open Stomp Manager](install/app.md)**, a desktop editor and live monitor for Linux, macOS and Windows. The
  vendor's *Stomp Manager* only runs on macOS and Windows.
* **[open-stomp](install/rust.md)**, the library it is built on: the pedal's USB-MIDI protocol, reverse engineered,
  as a Rust crate with [Python bindings](install/python.md).
* **NAM and IR conversion** that sounds closer to the real amp model and cab than the official app's, measured on
  the pedal: [how it works](analysis/overview.md) and [the results](analysis/results.md).
* **[The protocol](lib/01-overview.md)**, documented in full, and [how it was recovered](lib/07-capture-methodology.md).

<div class="feature">
<p class="feature-tag">New in 0.5</p>
<h2 class="feature-title">NAM imports that sound like the model, low end included</h2>
<p>A bass DI re-amped through the pedal with four conversions of the same amp model, against the original model and
cab IR in software. open-stomp 0.5 is the closest of the four in 87% of the recording.</p>
<a href="analysis/session2.html"><img src="lib/images/nam-reamp-difference.png" alt="How far each NAM conversion is from the original model and IR, by frequency: open-stomp 0.5 stays within about 2 dB above 100 Hz, the official app's conversion is up to 17 dB too dark"></a>
<p><a class="feature-button" href="analysis/session2.html">Open the interactive comparison →</a>
<a class="feature-more" href="analysis/low-end.html">How the low end was fixed</a></p>
</div>

> Probably also works with the Dual Stomp and Tiny Stomp (same firmware family, same vendor app). Only the Dual Stomp
> Lite on firmware **V1.1.1** with Stomp Manager **1.0.2** has been tested.

![Open Stomp Manager](manager/images/main-window.png)

## What you can do

| | |
|---|---|
| **Edit the pedal** | Presets, models, blocks on/off, every knob, preset volume, save and rename, system settings, looper, drum machine, tuner |
| **Import NAM models** | Any `.nam` file into a user AMP slot, converted by fitting the pedal's filters to the model through a simulation of the pedal |
| **Import cab IRs** | Any WAV into a user IR slot, fitted to the 384 samples the pedal actually plays |
| **Watch the pedal** | Everything that happens on it, in plain English (`AMP · Mar J800 · GAIN = 64`) |
| **Write your own tools** | The same features from Rust or Python, plus capture replay so you can develop without a pedal |

## Where to start

* **Just want to edit your pedal?** [Install the app](install/app.md), then read the [user guide](manager/user-guide.md).
* **Writing code?** [Rust](install/rust.md) or [Python](install/python.md), then the
  [library guide](lib/10-library-guide.md).
* **Curious how the pedal works?** Start with the [protocol overview](lib/01-overview.md) and
  [NAM and IR uploads](lib/05-uploads-nam-ir.md).

## Source

* Library and these docs: [github.com/JackMechem/open-stomp-lib](https://github.com/JackMechem/open-stomp-lib)
* App: [github.com/JackMechem/open-stomp-manager](https://github.com/JackMechem/open-stomp-manager)

## Legal

This is an independent interoperability project. It is not affiliated with or endorsed by SORAN, NUX/Cherub or
JiYou. Product names are trademarks of their owners. The repositories contain **no vendor binaries or firmware**:
only documentation, our own code, and captures of traffic between the author's own pedal and app. Use at your own
risk: writing bad data to a pedal could corrupt presets. Back them up first.
