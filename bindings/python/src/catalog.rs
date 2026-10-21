//! `open_stomp.catalog`: modules, models, parameters, drum patterns, note names.

use open_stomp::catalog::{self as cat, ParamKind};
use pyo3::prelude::*;

/// A block in the signal chain. The value is the pedal-page CC value and, for the eight
/// effect modules, also the head CC number.
#[pyclass(eq, eq_int, frozen, hash, from_py_object, module = "open_stomp.catalog")]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Module {
    #[pyo3(name = "OD_FX")]
    OdFx = 0,
    #[pyo3(name = "AMP")]
    Amp = 1,
    #[pyo3(name = "CAB")]
    Cab = 2,
    #[pyo3(name = "MOD")]
    Mod = 3,
    #[pyo3(name = "DELAY")]
    Delay = 4,
    #[pyo3(name = "REVERB")]
    Reverb = 5,
    #[pyo3(name = "GATE")]
    Gate = 6,
    #[pyo3(name = "EQ")]
    Eq = 7,
    #[pyo3(name = "LOOP")]
    Loop = 8,
    #[pyo3(name = "DRUM")]
    Drum = 9,
}

impl From<cat::Module> for Module {
    fn from(m: cat::Module) -> Self {
        Module::from_index(m as u8).expect("same discriminants")
    }
}

impl From<Module> for cat::Module {
    fn from(m: Module) -> Self {
        cat::Module::from_index(m as u8).expect("same discriminants")
    }
}

impl Module {
    fn inner(self) -> cat::Module {
        self.into()
    }
}

#[pymethods]
impl Module {
    /// All ten blocks in page order.
    #[classattr]
    #[pyo3(name = "ALL")]
    fn all() -> Vec<Module> {
        cat::Module::ALL.into_iter().map(Module::from).collect()
    }

    /// The eight modules with a head CC, models and parameters.
    #[classattr]
    #[pyo3(name = "EFFECTS")]
    fn effects() -> Vec<Module> {
        cat::Module::EFFECTS.into_iter().map(Module::from).collect()
    }

    #[staticmethod]
    pub fn from_index(i: u8) -> Option<Module> {
        Some(match i {
            0 => Module::OdFx,
            1 => Module::Amp,
            2 => Module::Cab,
            3 => Module::Mod,
            4 => Module::Delay,
            5 => Module::Reverb,
            6 => Module::Gate,
            7 => Module::Eq,
            8 => Module::Loop,
            9 => Module::Drum,
            _ => return None,
        })
    }

    /// Which module and parameter slot a parameter CC belongs to.
    #[staticmethod]
    fn from_param_cc(cc: u8) -> Option<(Module, usize)> {
        cat::Module::from_param_cc(cc).map(|(m, s)| (m.into(), s))
    }

    /// Index into a preset's `modules` list (= the enum value).
    #[getter]
    fn index(&self) -> usize {
        self.inner().index()
    }

    /// Display name as on the pedal and in the official app, e.g. `"OD/FX"`.
    #[getter]
    fn label(&self) -> &'static str {
        self.inner().name()
    }

    #[getter]
    fn is_effect(&self) -> bool {
        self.inner().is_effect()
    }

    /// Head CC (model + on/off), `None` for LOOP/DRUM.
    #[getter]
    fn head_cc(&self) -> Option<u8> {
        self.inner().head_cc()
    }

    /// `(first parameter CC, number of parameter CCs)`, `None` for LOOP/DRUM.
    #[getter]
    fn param_ccs(&self) -> Option<(u8, u8)> {
        self.inner().param_ccs()
    }

    /// Model names, index = model number.
    #[getter]
    fn models(&self) -> Vec<&'static str> {
        self.inner().models().to_vec()
    }

    fn model_name(&self, model: u8) -> String {
        self.inner().model_name(model)
    }

    /// Parameters of a model in the official app's display order.
    fn params(&self, model: u8) -> Vec<ParamDef> {
        self.inner().params(model).into_iter().map(ParamDef).collect()
    }

    /// The parameter in a slot, if the model uses it.
    fn param(&self, model: u8, slot: usize) -> Option<ParamDef> {
        self.inner().param(model, slot).map(ParamDef)
    }

    fn __repr__(&self) -> String {
        format!("Module.{}", self.py_name())
    }

    fn __str__(&self) -> &'static str {
        self.inner().name()
    }
}

impl Module {
    fn py_name(self) -> &'static str {
        match self {
            Module::OdFx => "OD_FX",
            Module::Amp => "AMP",
            Module::Cab => "CAB",
            Module::Mod => "MOD",
            Module::Delay => "DELAY",
            Module::Reverb => "REVERB",
            Module::Gate => "GATE",
            Module::Eq => "EQ",
            Module::Loop => "LOOP",
            Module::Drum => "DRUM",
        }
    }
}

/// One parameter of a model.
#[pyclass(frozen, module = "open_stomp.catalog")]
#[derive(Clone)]
pub struct ParamDef(pub cat::ParamDef);

#[pymethods]
impl ParamDef {
    /// Offset from the module's first parameter CC (= index into `ModuleState.params`).
    #[getter]
    fn slot(&self) -> u8 {
        self.0.slot
    }

    #[getter]
    fn name(&self) -> &'static str {
        self.0.name
    }

    /// `"knob"` (0–100), `"bipolar"` (0–100, 50 = centre) or `"enum"` (index into `choices`).
    #[getter]
    fn kind(&self) -> &'static str {
        match self.0.kind {
            ParamKind::Knob => "knob",
            ParamKind::Bipolar => "bipolar",
            ParamKind::Enum(_) => "enum",
        }
    }

    /// The options of an `"enum"` parameter, else `None`.
    #[getter]
    fn choices(&self) -> Option<Vec<&'static str>> {
        match self.0.kind {
            ParamKind::Enum(o) => Some(o.to_vec()),
            _ => None,
        }
    }

    /// The value as the official app shows it (`"64"`, `"+10"`, `"SM57"`).
    fn format(&self, value: u8) -> String {
        self.0.format(value)
    }

    fn __repr__(&self) -> String {
        format!("ParamDef(slot={}, name={:?}, kind={:?})", self.0.slot, self.0.name, self.kind())
    }
}

/// Drum pattern name from the flat value (`style * 10 + pattern`), e.g. `"Rock 04"`.
#[pyfunction]
fn drum_pattern_name(value: u8) -> String {
    cat::drum_pattern_name(value)
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Module>()?;
    m.add_class::<ParamDef>()?;
    m.add_function(wrap_pyfunction!(drum_pattern_name, m)?)?;
    m.add("NOTES", cat::NOTES.to_vec())?;
    m.add("DRUM_STYLES", cat::DRUM_STYLES.to_vec())?;
    for (name, list) in [
        ("OD_MODELS", cat::OD_MODELS),
        ("AMP_MODELS", cat::AMP_MODELS),
        ("CAB_MODELS", cat::CAB_MODELS),
        ("MOD_MODELS", cat::MOD_MODELS),
        ("DELAY_MODELS", cat::DELAY_MODELS),
        ("REVERB_MODELS", cat::REVERB_MODELS),
        ("MICS", cat::MICS),
        ("POSITIONS", cat::POSITIONS),
    ] {
        m.add(name, list.to_vec())?;
    }
    Ok(())
}
