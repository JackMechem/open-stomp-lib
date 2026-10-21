"""The protocol layer against the captured session (same checks as the Rust tests)."""

from open_stomp import DeviceState, Module, Msg, Origin, Preset, Settings
from open_stomp.midi import host
from open_stomp.queue import RequestQueue, handshake


def messages(entries):
    return [(e.origin, e.bytes) for e in entries if e.kind == "message"]


def test_replay_main_session(main_session):
    st = DeviceState()
    unknown = 0
    checked = False
    for origin, b in messages(main_session):
        ev = st.apply(b, origin)
        unknown += ev.category == "unknown"
        if not checked and all(p is not None for p in st.presets):
            checked = True
            assert (st.firmware, st.model_id, st.product) == ("V1.1.1", "JME-01", "JME STOMPBOX")
            for i, p in enumerate(st.presets):
                assert p.index == i and p.name
                assert p.module(Module.AMP).model < 40
            assert st.live_known
    assert checked and unknown < 10
    assert st.settings.input_trim <= 20


def test_records_round_trip(main_session):
    presets = settings = 0
    for _, b in messages(main_session):
        m = Msg.parse(b)
        if m.kind != "nep" or m.dir not in (1, 2):
            continue
        if m.cmd in (0x0B, 0x0C):
            p = Preset.from_packed(m.payload)
            assert p.to_packed() == m.payload, p.name
            if m.dir == 1:
                assert host.preset_write(p) == b
            presets += 1
        elif m.cmd == 0x14:
            s = Settings.from_packed(m.payload)
            assert s.to_packed() == m.payload
            if m.dir == 1:
                assert host.settings_write(s) == b
            settings += 1
    assert presets >= 100 and settings > 100


def test_save_as_matches_capture(main_session):
    msgs = messages(main_session)
    at = next(i for i, (o, b) in enumerate(msgs) if o == Origin.HOST and len(b) > 5 and b[5] == 0x12)
    st = DeviceState()
    for o, b in msgs[:at]:
        st.apply(b, o)
    assert st.current == 0x44
    p = st.live
    p.index = 0x15
    p.name = "4Ooz!"
    theirs = [b for o, b in msgs[at:] if o == Origin.HOST][:3]
    assert host.save_preset(st.current, p) == theirs


def key(req: bytes) -> bytes:
    """A request without trailing zero padding (the official app pads some reads)."""
    return req[:-1].rstrip(b"\x00")


def test_sync_against_recorded_pedal(main_session):
    # fake pedal: request -> the first matching reply that followed it in the capture
    replies, waiting = {}, None
    for o, b in messages(main_session):
        if o == Origin.HOST:
            waiting = None
            if b[0] == 0xF0 and key(b) not in replies:
                waiting = RequestQueue()
                waiting.read(b)
                waiting.sent(waiting.pop())
                waiting_key = key(b)
        elif waiting is not None and waiting.on_message(b):
            replies[waiting_key] = b
            waiting = None

    q, st, now, timeouts = RequestQueue(timeout=1.0), DeviceState(), 0.0, 0
    q.start_sync(handshake())
    assert q.progress == (0, 127)
    while q.progress != (0, 0) or not q.is_empty:
        r = q.pop()
        st.apply(r.bytes, Origin.HOST)
        q.sent(r, at=now)
        reply = replies.get(key(r.bytes))
        if reply:
            assert q.on_message(reply).answered
            st.apply(reply, Origin.PEDAL)
        else:
            now += 1.1
            assert not q.expire(at=now).answered
            timeouts += 1
    assert timeouts <= 1  # the tuner is never read in the capture
    assert st.firmware == "V1.1.1" and all(p is not None for p in st.presets)
    assert st.live_known and st.live.name == "SV Bass"
