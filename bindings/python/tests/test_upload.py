"""Upload framing against the captured NAM and IR imports."""

import pytest

from open_stomp import Msg, Origin, upload
from open_stomp.upload import Chunk, Reassembler, UserAmpInfo


def host_neps(entries):
    out = []
    for e in entries:
        if e.kind == "message" and e.origin == Origin.HOST:
            m = Msg.parse(e.bytes)
            if m.kind == "nep" and m.dir == 1 and m.cmd in (0x58, 0x10, 0x09):
                out.append((m, e.bytes))
    return out


def test_nam_imports_rebuild(main_session):
    msgs = host_neps(main_session)
    imports = 0
    for i, (m, b) in enumerate(msgs):
        info = UserAmpInfo.parse(m.payload) if m.cmd == 0x58 else None
        if info is None:
            continue
        r, got, end = Reassembler(), None, i
        while got is None:
            end += 1
            got = r.push(Chunk.parse(msgs[end][0].payload))
        slot, floats = got
        assert (slot, len(floats), info.factor) == (info.slot, upload.UPLOAD_FLOATS, 1.0)
        ours = upload.user_amp_upload(UserAmpInfo(info.slot, info.name, info.factor), floats)
        assert ours == [b for _, b in msgs[i : end + 1]]
        imports += 1
    assert imports == 3


def test_ir_import_rebuild(main_session):
    msgs = host_neps(main_session)
    start = next(i for i, (m, _) in enumerate(msgs) if m.cmd == 0x10)
    r, got, end = Reassembler(), None, start - 1
    while got is None:
        end += 1
        got = r.push(Chunk.parse(msgs[end][0].payload))
    slot, floats = got
    ours = upload.user_ir_upload(slot, "Delay And Reverb", floats)
    assert ours == [b for _, b in msgs[start : end + 2]]


def test_check_and_info():
    upload.check(9, [0.0] * 2048)
    for slot, data in ((10, [0.0] * 2048), (0, [0.0] * 2047), (0, [float("nan")] * 2048)):
        with pytest.raises(ValueError):
            upload.check(slot, data)
    i = UserAmpInfo(2, "Jcm800")
    p = UserAmpInfo.parse(Msg.parse(i.write_frame()).payload)
    assert (p.slot, p.name, p.factor) == (2, "Jcm800", 1.0)
    assert upload.bytes_to_floats(upload.floats_to_bytes([1.5, -2.0])) == [1.5, -2.0]
