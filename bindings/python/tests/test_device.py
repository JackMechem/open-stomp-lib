"""The engine thread end to end through capture replay (no pedal needed)."""

import time

import pytest

from open_stomp import Device, DeviceState, Origin


def test_replay(main_log):
    st = DeviceState()
    seen = set()
    with Device(auto_connect=False) as dev:
        dev.replay(str(main_log), speed=1000.0)
        deadline = time.monotonic() + 30
        while time.monotonic() < deadline:
            u = dev.recv(timeout=1.0)
            if u is None:
                continue
            seen.add(u.kind)
            if u.kind == "message":
                assert u.origin in (Origin.PEDAL, Origin.HOST) and u.at > 1.7e9
                assert st.feed(u) is not None
            if u.kind == "info" and u.text == "Replay finished":
                break
        else:
            pytest.fail("replay did not finish")
    assert {"reset", "status", "message", "mark"} <= seen
    assert st.firmware == "V1.1.1"
    assert dev.closed
    with pytest.raises(RuntimeError):
        dev.select_preset(1)
    assert list(dev) == []


def test_recv_timeout_and_drain():
    dev = Device(auto_connect=False, sync=False)
    t = time.perf_counter()  # time.monotonic() ticks every ~16 ms on Windows
    while dev.recv(timeout=0.2) is not None:  # initial ports / info updates
        pass
    assert time.perf_counter() - t >= 0.19
    assert dev.drain() == [] and dev.try_recv() is None
    dev.close()
    dev.close()
