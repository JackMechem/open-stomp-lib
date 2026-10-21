"""NAM inference and conversion from Python (the Rust tests cover accuracy in depth)."""

from pathlib import Path

import pytest

from open_stomp import nam

DATA = Path(__file__).resolve().parents[3] / "tests" / "data" / "nam"


def f32(path):
    import array

    a = array.array("f")
    a.frombytes(path.read_bytes())
    return a


def test_inference_matches_neuralampmodelercore():
    m = nam.Model.load(DATA / "wavenet.nam")
    assert m.architecture == "WaveNet" and m.sample_rate == 48000.0
    x, want = f32(DATA / "input.f32"), f32(DATA / "wavenet.out.f32")
    y = m.process(x)
    assert max(abs(a - b) for a, b in zip(y, want)) < 1e-4
    m.reset()
    assert m.process(x[:100]) == y[:100]


def test_convert():
    c = nam.convert(DATA / "wavenet.nam")
    assert len(c.data) == 2048 and c.factor == 1.0
    assert len(c.levels) == len(nam.TONE_BINS) == 94
    assert c.data == nam.convert(nam.Model.load(DATA / "wavenet.nam")).data
    with pytest.raises(ValueError):
        nam.convert(DATA / "wavenet.nam", excitation=[0.0] * 10)
    assert len(nam.excitation()) == nam.FFT_SIZE


def test_errors():
    with pytest.raises(OSError):
        nam.Model.load("/no/such/model.nam")
    with pytest.raises(ValueError):
        nam.Model.from_json('{"architecture": "LSTM", "config": {}, "weights": []}')
