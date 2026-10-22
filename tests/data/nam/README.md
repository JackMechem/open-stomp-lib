# NAM inference test vectors

`open_stomp::nam` is a port of [NeuralAmpModelerCore](https://github.com/sdatkinson/NeuralAmpModelerCore).
`tests/nam_inference.rs` runs these models on `input.f32` and compares with NeuralAmpModelerCore's output.

| File | |
|---|---|
| `wavenet.nam`, `slimmable_container.nam`, `A2.nam`, `wavenet_a1_standard.nam` | Example models from NeuralAmpModelerCore (`example_models/`, commit 0b3d3c97b085), MIT licence: `LICENSE-NeuralAmpModelerCore` |
| `input.f32` | 12000 samples, float32 LE, 48 kHz: 3000 of noise at −40 dBFS, a 20 Hz–20 kHz exponential sweep at −6 dBFS, 2000 of loud uniform noise, 1000 of silence (numpy seed 20261021) |
| `<model>.out.f32` | NeuralAmpModelerCore's output for `input.f32`, rendered with `nam_ref.cpp` (`ResetAndPrewarm`, blocks of 1000, float samples) |

`A2.nam` and `slimmable_container.nam` are `SlimmableContainer` models of the same architecture family the vendor's converter
handles; `wavenet_a1_standard.nam` is the classic "standard" NAM WaveNet most downloadable models use.

To regenerate (NeuralAmpModelerCore checkout with its submodules):

```sh
clang++ -std=c++20 -O2 -DNAM_SAMPLE_FLOAT -I. -IDependencies/eigen -IDependencies/nlohmann -INAM \
  -IDependencies/AudioDSPTools/dsp -IDependencies/AudioDSPTools -o nam_ref nam_ref.cpp NAM/*.cpp NAM/*/*.cpp \
  Dependencies/AudioDSPTools/dsp/wav.cpp
./nam_ref A2.nam input.f32 A2.out.f32 1000
```
