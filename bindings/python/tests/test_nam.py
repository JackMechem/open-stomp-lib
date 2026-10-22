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


def test_convert_measure():
    clean = nam.convert(DATA / "wavenet.nam", measure="clean")
    # only the taps the pedal runs
    assert all(v == 0.0 for v in clean.data[:512] + clean.data[1024:1536])
    with pytest.raises(ValueError):
        nam.convert(DATA / "wavenet.nam", measure="exact")


def test_errors():
    with pytest.raises(OSError):
        nam.Model.load("/no/such/model.nam")
    with pytest.raises(ValueError):
        nam.Model.from_json('{"architecture": "LSTM", "config": {}, "weights": []}')


def test_official_excitation_when_installed():
    try:
        x = nam.official_excitation()
    except ValueError:
        pytest.skip("Stomp Manager is not installed")
    assert len(x) == nam.FFT_SIZE and max(abs(v) for v in x) < 1e-4
    with pytest.raises(ValueError):
        nam.official_excitation(DATA)          # no table in the test data


def test_level_normalisation():
    m = nam.Model.load(DATA / "wavenet.nam")
    raw, norm = nam.convert(m), nam.convert(m, normalise=True)
    assert raw.data[:1024] == norm.data[:1024]           # G1 untouched: a pure volume change
    gain_db = nam.NORMALISED_LEVEL_DB - nam.pedal_level_db(m)
    ratio = norm.data[2047] / raw.data[2047]
    assert ratio == pytest.approx(10 ** (gain_db / 20), rel=1e-5)
