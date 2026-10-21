//! `open_stomp.midi` (+ `.req`, `.host`) and `open_stomp.pack`: messages, framing, builders.

use open_stomp::midi::{self as m, host as h, req as r};
use pyo3::prelude::*;
use pyo3::types::PyBytes;

use crate::catalog::Module;
use crate::records::{Preset, Settings};
use crate::util::{bytes, bytes_arg, bytes_list};

/// One parsed MIDI message. `kind` says which fields are set:
///
/// | kind | fields |
/// |---|---|
/// | `"cc"` | `ch`, `cc`, `val` |
/// | `"pc"` | `ch`, `prog` |
/// | `"nep"` | `cmd`, `dir`, `payload` (`F0 00 21 7A 70 <cmd> <dir> <payload> F7`) |
/// | `"notify"` | `what`, `payload` (`… 70 7E 02 <what> 02 <payload> F7`) |
/// | `"short"` | `short_kind`, `payload` (`F0 00 21 7A <kind> <payload> F7`) |
/// | `"other"` | `payload` (the whole message) |
#[pyclass(frozen, module = "open_stomp.midi")]
#[derive(Clone)]
pub struct Msg(pub m::Msg);

#[pymethods]
impl Msg {
    #[staticmethod]
    fn parse(#[pyo3(from_py_with = bytes_arg)] data: Vec<u8>) -> Msg {
        Msg(m::Msg::parse(&data))
    }

    #[getter]
    fn kind(&self) -> &'static str {
        match self.0 {
            m::Msg::Cc { .. } => "cc",
            m::Msg::Pc { .. } => "pc",
            m::Msg::Nep { .. } => "nep",
            m::Msg::Notify { .. } => "notify",
            m::Msg::Short { .. } => "short",
            m::Msg::Other(_) => "other",
        }
    }

    /// MIDI channel, 0-based (0 = pedal → host, 1 = host → pedal).
    #[getter]
    fn ch(&self) -> Option<u8> {
        match self.0 {
            m::Msg::Cc { ch, .. } | m::Msg::Pc { ch, .. } => Some(ch),
            _ => None,
        }
    }

    #[getter]
    fn cc(&self) -> Option<u8> {
        match self.0 {
            m::Msg::Cc { cc, .. } => Some(cc),
            _ => None,
        }
    }

    #[getter]
    fn val(&self) -> Option<u8> {
        match self.0 {
            m::Msg::Cc { val, .. } => Some(val),
            _ => None,
        }
    }

    #[getter]
    fn prog(&self) -> Option<u8> {
        match self.0 {
            m::Msg::Pc { prog, .. } => Some(prog),
            _ => None,
        }
    }

    #[getter]
    fn cmd(&self) -> Option<u8> {
        match self.0 {
            m::Msg::Nep { cmd, .. } => Some(cmd),
            _ => None,
        }
    }

    /// 0 read, 1 write, 2 reply, 3 ack.
    #[getter]
    fn dir(&self) -> Option<u8> {
        match self.0 {
            m::Msg::Nep { dir, .. } => Some(dir),
            _ => None,
        }
    }

    #[getter]
    fn what(&self) -> Option<u8> {
        match self.0 {
            m::Msg::Notify { what, .. } => Some(what),
            _ => None,
        }
    }

    #[getter]
    fn short_kind(&self) -> Option<u8> {
        match self.0 {
            m::Msg::Short { kind, .. } => Some(kind),
            _ => None,
        }
    }

    #[getter]
    fn payload<'py>(&self, py: Python<'py>) -> Option<Bound<'py, PyBytes>> {
        match &self.0 {
            m::Msg::Nep { payload, .. } | m::Msg::Notify { payload, .. } | m::Msg::Short { payload, .. } => {
                Some(bytes(py, payload))
            }
            m::Msg::Other(b) => Some(bytes(py, b)),
            _ => None,
        }
    }

    fn __eq__(&self, other: &Self) -> bool {
        self.0 == other.0
    }

    fn __repr__(&self) -> String {
        match &self.0 {
            m::Msg::Cc { ch, cc, val } => format!("Msg(kind='cc', ch={ch}, cc=0x{cc:02X}, val={val})"),
            m::Msg::Pc { ch, prog } => format!("Msg(kind='pc', ch={ch}, prog={prog})"),
            m::Msg::Nep { cmd, dir, payload } => {
                format!("Msg(kind='nep', cmd=0x{cmd:02X}, dir={dir}, payload=<{} bytes>)", payload.len())
            }
            m::Msg::Notify { what, payload } => {
                format!("Msg(kind='notify', what=0x{what:02X}, payload=<{} bytes>)", payload.len())
            }
            m::Msg::Short { kind, payload } => {
                format!("Msg(kind='short', short_kind=0x{kind:02X}, payload=<{} bytes>)", payload.len())
            }
            m::Msg::Other(b) => format!("Msg(kind='other', payload={})", m::hex(b)),
        }
    }
}

/// Reassembles a raw byte stream (possibly split SysEx, any chunking) into whole messages.
#[pyclass(module = "open_stomp.midi")]
#[derive(Default)]
pub struct Assembler(m::Assembler);

#[pymethods]
impl Assembler {
    #[new]
    fn new() -> Self {
        Self::default()
    }

    /// Feed bytes; returns the messages completed by them.
    fn push<'py>(&mut self, py: Python<'py>, #[pyo3(from_py_with = bytes_arg)] data: Vec<u8>) -> Vec<Bound<'py, PyBytes>> {
        bytes_list(py, &self.0.push(&data))
    }
}

/// `"F0 00 21 7A …"`
#[pyfunction]
fn hex(#[pyo3(from_py_with = bytes_arg)] data: Vec<u8>) -> String {
    m::hex(&data)
}

/// Build a NEP frame `F0 00 21 7A 70 <cmd> <dir> <payload> F7` (payload bytes must be < 0x80).
#[pyfunction]
fn frame<'py>(py: Python<'py>, cmd: u8, dir: u8, #[pyo3(from_py_with = bytes_arg)] payload: Vec<u8>) -> Bound<'py, PyBytes> {
    let mut v = vec![0xF0, m::MFR[0], m::MFR[1], m::MFR[2], m::UID, cmd & 0x7F, dir & 0x7F];
    v.extend(payload.iter().map(|b| b & 0x7F));
    v.push(0xF7);
    bytes(py, &v)
}

// ───────────────────────────── req ─────────────────────────────

macro_rules! req_fns {
    ($($name:ident),*) => {$(
        #[pyfunction]
        fn $name<'py>(py: Python<'py>) -> Bound<'py, PyBytes> {
            bytes(py, &r::$name())
        }
    )*};
}
req_fns!(version, product, settings, cur_state, drum, looper, tuner, edit_buffer);

#[pyfunction]
fn ir_slot<'py>(py: Python<'py>, slot: u8) -> Bound<'py, PyBytes> {
    bytes(py, &r::ir_slot(slot))
}

#[pyfunction]
fn preset<'py>(py: Python<'py>, index: u8) -> Bound<'py, PyBytes> {
    bytes(py, &r::preset(index))
}

#[pyfunction]
fn user_amp<'py>(py: Python<'py>, slot: u8) -> Bound<'py, PyBytes> {
    bytes(py, &r::user_amp(slot))
}

// ───────────────────────────── host ─────────────────────────────

#[pyfunction]
fn cc<'py>(py: Python<'py>, cc: u8, val: u8) -> Bound<'py, PyBytes> {
    bytes(py, &h::cc(cc, val))
}

#[pyfunction]
fn program<'py>(py: Python<'py>, index: u8) -> Bound<'py, PyBytes> {
    bytes(py, &h::program(index))
}

/// Model + on/off of an effect module; `None` for LOOP/DRUM.
#[pyfunction]
fn head<'py>(py: Python<'py>, module: Module, model: u8, on: bool) -> Option<Bound<'py, PyBytes>> {
    h::head(module.into(), model, on).map(|b| bytes(py, &b))
}

/// A module parameter by slot; `None` if the module has no such slot.
#[pyfunction]
fn param<'py>(py: Python<'py>, module: Module, slot: usize, value: u8) -> Option<Bound<'py, PyBytes>> {
    h::param(module.into(), slot, value).map(|b| bytes(py, &b))
}

#[pyfunction]
fn volume<'py>(py: Python<'py>, value: u8) -> Bound<'py, PyBytes> {
    bytes(py, &h::volume(value))
}

#[pyfunction]
fn page<'py>(py: Python<'py>, module: Module) -> Bound<'py, PyBytes> {
    bytes(py, &h::page(module.into()))
}

#[pyfunction]
fn looper_button<'py>(py: Python<'py>, button: u8) -> Bound<'py, PyBytes> {
    bytes(py, &h::looper_button(button))
}

#[pyfunction]
fn preset_name<'py>(py: Python<'py>, source_slot: u8, name: &str) -> Bound<'py, PyBytes> {
    bytes(py, &h::preset_name(source_slot, name))
}

#[pyfunction]
fn preset_write<'py>(py: Python<'py>, preset: PyRef<'_, Preset>) -> Bound<'py, PyBytes> {
    bytes(py, &h::preset_write(&preset.0))
}

/// The official app's save sequence: name, record, select the saved slot.
#[pyfunction]
fn save_preset<'py>(py: Python<'py>, source_slot: u8, preset: PyRef<'_, Preset>) -> Vec<Bound<'py, PyBytes>> {
    bytes_list(py, &h::save_preset(source_slot, &preset.0))
}

#[pyfunction]
fn settings_write<'py>(py: Python<'py>, settings: PyRef<'_, Settings>) -> Bound<'py, PyBytes> {
    bytes(py, &h::settings_write(&settings.0))
}

// ───────────────────────────── pack ─────────────────────────────

/// 8-bit struct bytes → 7-bit MIDI bytes (LSB-first bitstream).
#[pyfunction]
fn pack<'py>(py: Python<'py>, #[pyo3(from_py_with = bytes_arg)] data: Vec<u8>) -> Bound<'py, PyBytes> {
    bytes(py, &open_stomp::pack::pack(&data))
}

/// 7-bit MIDI bytes → 8-bit struct bytes (a trailing partial byte is dropped).
#[pyfunction]
fn unpack<'py>(py: Python<'py>, #[pyo3(from_py_with = bytes_arg)] data: Vec<u8>) -> Bound<'py, PyBytes> {
    bytes(py, &open_stomp::pack::unpack(&data))
}

pub fn register_pack(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(pack, m)?)?;
    m.add_function(wrap_pyfunction!(unpack, m)?)?;
    Ok(())
}

pub fn register(py: Python<'_>, md: &Bound<'_, PyModule>) -> PyResult<()> {
    md.add_class::<Msg>()?;
    md.add_class::<Assembler>()?;
    md.add_function(wrap_pyfunction!(hex, md)?)?;
    md.add_function(wrap_pyfunction!(frame, md)?)?;
    md.add("MFR", PyBytes::new(py, &m::MFR))?;
    md.add("UID", m::UID)?;
    md.add("PEDAL_CH", m::PEDAL_CH)?;
    md.add("HOST_CH", m::HOST_CH)?;

    let req = PyModule::new(py, "req")?;
    for f in [
        wrap_pyfunction!(version, &req)?,
        wrap_pyfunction!(product, &req)?,
        wrap_pyfunction!(settings, &req)?,
        wrap_pyfunction!(cur_state, &req)?,
        wrap_pyfunction!(drum, &req)?,
        wrap_pyfunction!(looper, &req)?,
        wrap_pyfunction!(tuner, &req)?,
        wrap_pyfunction!(edit_buffer, &req)?,
        wrap_pyfunction!(ir_slot, &req)?,
        wrap_pyfunction!(preset, &req)?,
        wrap_pyfunction!(user_amp, &req)?,
    ] {
        req.add_function(f)?;
    }
    md.add_submodule(&req)?;

    let host = PyModule::new(py, "host")?;
    for f in [
        wrap_pyfunction!(cc, &host)?,
        wrap_pyfunction!(program, &host)?,
        wrap_pyfunction!(head, &host)?,
        wrap_pyfunction!(param, &host)?,
        wrap_pyfunction!(volume, &host)?,
        wrap_pyfunction!(page, &host)?,
        wrap_pyfunction!(looper_button, &host)?,
        wrap_pyfunction!(preset_name, &host)?,
        wrap_pyfunction!(preset_write, &host)?,
        wrap_pyfunction!(save_preset, &host)?,
        wrap_pyfunction!(settings_write, &host)?,
    ] {
        host.add_function(f)?;
    }
    host.add("LOOP_REC_PLAY_DUB", h::LOOP_REC_PLAY_DUB)?;
    host.add("LOOP_STOP", h::LOOP_STOP)?;
    host.add("LOOP_CLEAR", h::LOOP_CLEAR)?;
    host.add("LOOP_UNDO_REDO", h::LOOP_UNDO_REDO)?;
    md.add_submodule(&host)?;
    Ok(())
}
