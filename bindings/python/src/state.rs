//! `open_stomp.state`: the pedal model every message is applied to.

use open_stomp::state as st;
use pyo3::prelude::*;
use pyo3::types::PyBytes;

use crate::catalog::Module;
use crate::records::{Preset, Settings, py_bool};
use crate::util::{bytes, bytes_arg};

/// Direction of a message.
#[pyclass(eq, eq_int, frozen, hash, from_py_object, module = "open_stomp.state")]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Origin {
    /// Pedal → host.
    #[pyo3(name = "PEDAL")]
    Pedal,
    /// Host → pedal (this program, or the official app in a replayed capture).
    #[pyo3(name = "HOST")]
    Host,
}

impl From<st::Origin> for Origin {
    fn from(o: st::Origin) -> Self {
        match o {
            st::Origin::Pedal => Origin::Pedal,
            st::Origin::Host => Origin::Host,
        }
    }
}

impl From<Origin> for st::Origin {
    fn from(o: Origin) -> Self {
        match o {
            Origin::Pedal => st::Origin::Pedal,
            Origin::Host => st::Origin::Host,
        }
    }
}

/// A human-readable description of one MIDI message.
#[pyclass(frozen, module = "open_stomp.state")]
pub struct Event(pub st::Event);

#[pymethods]
impl Event {
    #[getter]
    fn origin(&self) -> Origin {
        self.0.origin.into()
    }

    /// `"preset"`, `"effect"`, `"volume"`, `"navigation"`, `"looper"`, `"drum"`, `"tuner"`,
    /// `"settings"`, `"device"`, `"protocol"` or `"unknown"`.
    #[getter]
    fn category(&self) -> &'static str {
        match self.0.category {
            st::Category::Preset => "preset",
            st::Category::Effect(_) => "effect",
            st::Category::Volume => "volume",
            st::Category::Navigation => "navigation",
            st::Category::Looper => "looper",
            st::Category::Drum => "drum",
            st::Category::Tuner => "tuner",
            st::Category::Settings => "settings",
            st::Category::Device => "device",
            st::Category::Protocol => "protocol",
            st::Category::Unknown => "unknown",
        }
    }

    /// The effect module for `category == "effect"`.
    #[getter]
    fn module(&self) -> Option<Module> {
        match self.0.category {
            st::Category::Effect(m) => Some(m.into()),
            _ => None,
        }
    }

    /// Short display label of the category, e.g. `"AMP"`, `"LOOPER"`, `"SYSEX"`.
    #[getter]
    fn label(&self) -> &'static str {
        self.0.category.label()
    }

    /// What changed, e.g. `"Mar J800 · GAIN"`.
    #[getter]
    fn subject(&self) -> &str {
        &self.0.subject
    }

    /// The new value, e.g. `"64"`.
    #[getter]
    fn value(&self) -> &str {
        &self.0.value
    }

    /// Consecutive events with the same key are one gesture (a knob sweep).
    #[getter]
    fn coalesce(&self) -> Option<u64> {
        self.0.coalesce
    }

    /// The message changed nothing (a repeat of a known value, an unused parameter slot).
    #[getter]
    fn redundant(&self) -> bool {
        self.0.redundant
    }

    #[getter]
    fn raw<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        bytes(py, &self.0.raw)
    }

    fn __str__(&self) -> String {
        format!("{:8} {} = {}", self.0.category.label(), self.0.subject, self.0.value)
    }

    fn __repr__(&self) -> String {
        format!(
            "Event(origin={:?}, category={:?}, subject={:?}, value={:?}, redundant={})",
            self.0.origin,
            self.category(),
            self.0.subject,
            self.0.value,
            py_bool(self.0.redundant)
        )
    }
}

/// Looper state snapshot.
#[pyclass(frozen, get_all, module = "open_stomp.state")]
pub struct Looper {
    /// 0 empty, 1 recording, 2 playing, 3 overdubbing, 4 stopped.
    state: u8,
    /// `"Empty"`, `"Recording"`, `"Playing"`, `"Overdubbing"`, `"Stopped"`.
    state_name: &'static str,
    /// 1 = overdub layer on top (undo available), 2 = base layer only.
    layers: u8,
    rec_level: Option<u8>,
    play_level: Option<u8>,
    sync: Option<bool>,
    /// 0 NR, 1 AR.
    mode: Option<u8>,
}

#[pymethods]
impl Looper {
    fn __repr__(&self) -> String {
        format!("Looper(state={:?}, layers={}, rec_level={:?}, play_level={:?})", self.state_name, self.layers, self.rec_level, self.play_level)
    }
}

/// Drum machine snapshot.
#[pyclass(frozen, get_all, module = "open_stomp.state")]
pub struct Drum {
    playing: bool,
    /// Flat pattern value (`style * 10 + pattern`), see `catalog.drum_pattern_name`.
    pattern: Option<u8>,
    level: Option<u8>,
    bpm: Option<u16>,
}

#[pymethods]
impl Drum {
    fn __repr__(&self) -> String {
        format!("Drum(playing={}, pattern={:?}, level={:?}, bpm={:?})", py_bool(self.playing), self.pattern, self.level, self.bpm)
    }
}

/// Tuner snapshot.
#[pyclass(frozen, get_all, module = "open_stomp.state")]
pub struct Tuner {
    active: bool,
    detected: bool,
    /// 0–11, index into `catalog.NOTES`.
    note: Option<u8>,
    /// Raw, 50 = in tune.
    cents: Option<u8>,
    /// Cents off pitch, negative = flat.
    cents_offset: Option<i16>,
}

#[pymethods]
impl Tuner {
    fn __repr__(&self) -> String {
        format!("Tuner(active={}, note={:?}, cents_offset={:?})", py_bool(self.active), self.note, self.cents_offset)
    }
}

/// A model of the pedal. Apply every complete message (either direction) with `apply`.
///
/// Attributes return copies (snapshots); the model only changes through `apply` and
/// `select_preset`.
#[pyclass(module = "open_stomp.state")]
#[derive(Default)]
pub struct DeviceState(pub st::DeviceState);

#[pymethods]
impl DeviceState {
    #[new]
    fn new() -> Self {
        Self::default()
    }

    /// Apply one complete MIDI message and describe it.
    fn apply(&mut self, #[pyo3(from_py_with = bytes_arg)] data: Vec<u8>, origin: Origin) -> Event {
        Event(self.0.apply(&data, origin.into()))
    }

    /// Apply an engine `Update`: a `"message"` is applied and its `Event` returned, a `"reset"`
    /// clears the model; other kinds are ignored (returns `None`).
    fn feed(&mut self, py: Python<'_>, update: PyRef<'_, crate::device::Update>) -> Option<Event> {
        match update.kind() {
            "reset" => {
                self.0 = st::DeviceState::default();
                None
            }
            "message" => {
                let (b, o) = update.message()?;
                Some(Event(self.0.apply(b.as_bytes(py), o.into())))
            }
            _ => None,
        }
    }

    /// Make `index` the active preset, loading its stored record into the edit buffer.
    fn select_preset(&mut self, index: u8) {
        self.0.select_preset(index);
    }

    /// The stored preset in a slot, if read.
    fn stored(&self, index: u8) -> Option<Preset> {
        self.0.stored(index).cloned().map(Preset)
    }

    fn preset_name(&self, index: u8) -> Option<String> {
        self.0.preset_name(index).map(str::to_owned)
    }

    /// Model name for display, including user NAM/IR slot names for user models.
    fn model_name(&self, module: Module, model: u8) -> String {
        self.0.model_name(module.into(), model)
    }

    /// The live edit buffer differs from the stored preset.
    fn modified(&self) -> bool {
        self.0.modified()
    }

    #[getter]
    fn firmware(&self) -> Option<String> {
        self.0.firmware.clone()
    }

    #[getter]
    fn model_id(&self) -> Option<String> {
        self.0.model_id.clone()
    }

    #[getter]
    fn product(&self) -> Option<String> {
        self.0.product.clone()
    }

    /// All 99 slots, `None` where not read yet.
    #[getter]
    fn presets(&self) -> Vec<Option<Preset>> {
        self.0.presets.iter().map(|p| p.clone().map(Preset)).collect()
    }

    /// Active preset index, 0-based.
    #[getter]
    fn current(&self) -> Option<u8> {
        self.0.current
    }

    /// The live edit buffer: the stored preset plus every change since.
    #[getter]
    fn live(&self) -> Preset {
        Preset(self.0.live.clone())
    }

    /// Whether `live` has been read (or follows a known stored preset).
    #[getter]
    fn live_known(&self) -> bool {
        self.0.live_known
    }

    /// The page the pedal's display shows.
    #[getter]
    fn page(&self) -> Option<Module> {
        self.0.page.map(Module::from)
    }

    #[getter]
    fn looper(&self) -> Looper {
        let l = &self.0.looper;
        Looper {
            state: l.state,
            state_name: l.state_name(),
            layers: l.layers,
            rec_level: l.rec_level,
            play_level: l.play_level,
            sync: l.sync,
            mode: l.mode,
        }
    }

    #[getter]
    fn drum(&self) -> Drum {
        let d = &self.0.drum;
        Drum { playing: d.playing, pattern: d.pattern, level: d.level, bpm: d.bpm }
    }

    #[getter]
    fn tuner(&self) -> Tuner {
        let t = &self.0.tuner;
        Tuner { active: t.active, detected: t.detected, note: t.note, cents: t.cents, cents_offset: t.cents_offset() }
    }

    #[getter]
    fn settings(&self) -> Option<Settings> {
        self.0.settings.clone().map(Settings)
    }

    /// Names of the 10 user AMP (NAM) slots, `None` where not read.
    #[getter]
    fn user_amps(&self) -> Vec<Option<String>> {
        self.0.user_amps.to_vec()
    }

    /// Names of the 10 user CAB (IR) slots, `None` where not read.
    #[getter]
    fn user_irs(&self) -> Vec<Option<String>> {
        self.0.user_irs.to_vec()
    }

    fn __repr__(&self) -> String {
        let read = self.0.presets.iter().filter(|p| p.is_some()).count();
        format!(
            "DeviceState(product={:?}, firmware={:?}, presets_read={read}, current={:?})",
            self.0.product, self.0.firmware, self.0.current
        )
    }
}

#[pyfunction]
fn looper_state_name(state: u8) -> &'static str {
    st::looper_state_name(state)
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Origin>()?;
    m.add_class::<Event>()?;
    m.add_class::<Looper>()?;
    m.add_class::<Drum>()?;
    m.add_class::<Tuner>()?;
    m.add_class::<DeviceState>()?;
    m.add_function(wrap_pyfunction!(looper_state_name, m)?)?;
    Ok(())
}
