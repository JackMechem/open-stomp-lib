//! `open_stomp.records`: preset and settings records (docs/04).

use open_stomp::records as rec;
use pyo3::exceptions::{PyIndexError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyBytes;

use crate::catalog::Module;
use crate::util::{bytes, bytes_arg};

/// One effect module's state inside a preset: on/off, model and its 12 parameter slots.
#[pyclass(from_py_object, module = "open_stomp.records")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModuleState {
    #[pyo3(get, set)]
    pub on: bool,
    #[pyo3(get, set)]
    pub model: u8,
    params: [u8; 12],
}

impl From<rec::ModuleState> for ModuleState {
    fn from(s: rec::ModuleState) -> Self {
        ModuleState { on: s.on, model: s.model, params: s.params }
    }
}

impl From<&ModuleState> for rec::ModuleState {
    fn from(s: &ModuleState) -> Self {
        rec::ModuleState { on: s.on, model: s.model, params: s.params }
    }
}

#[pymethods]
impl ModuleState {
    #[new]
    #[pyo3(signature = (on = false, model = 0, params = None))]
    fn new(on: bool, model: u8, params: Option<Vec<u8>>) -> PyResult<Self> {
        let mut s = ModuleState { on, model, params: [0; 12] };
        if let Some(p) = params {
            s.set_params(p)?;
        }
        Ok(s)
    }

    /// The 12 parameter slots (only the first few are used by any model, see `Module.params`).
    #[getter]
    fn params(&self) -> Vec<u8> {
        self.params.to_vec()
    }

    #[setter]
    fn set_params(&mut self, p: Vec<u8>) -> PyResult<()> {
        if p.len() > 12 {
            return Err(PyValueError::new_err("at most 12 parameter values"));
        }
        self.params = [0; 12];
        self.params[..p.len()].copy_from_slice(&p);
        Ok(())
    }

    /// The head CC value: `(0x40 if off else 0) | model`.
    #[getter]
    fn head(&self) -> u8 {
        rec::ModuleState::from(self).head()
    }

    fn __eq__(&self, other: &Self) -> bool {
        self == other
    }

    fn __repr__(&self) -> String {
        format!("ModuleState(on={}, model={}, params={:?})", py_bool(self.on), self.model, self.params)
    }
}

pub fn py_bool(b: bool) -> &'static str {
    if b { "True" } else { "False" }
}

/// A preset (SysEx `0x0B` / `0x0C`). Keeps the full 110-byte record so unknown bytes survive
/// a decode/encode round trip.
///
/// Attributes return copies: change a module with `set_module` / `set_param`, or assign a
/// whole list back to `modules`.
#[pyclass(from_py_object, module = "open_stomp.records")]
#[derive(Clone)]
pub struct Preset(pub rec::Preset);

fn check_effect(m: Module) -> PyResult<usize> {
    if m.is_effect_module() {
        Ok(m as usize)
    } else {
        Err(PyValueError::new_err(format!("{m:?} has no module state in a preset")))
    }
}

impl Module {
    fn is_effect_module(self) -> bool {
        (self as u8) < 8
    }
}

#[pymethods]
impl Preset {
    /// An empty preset (all modules off, volume 80, default chain).
    #[new]
    fn new() -> Self {
        Preset(rec::Preset::default())
    }

    /// Decode a packed SysEx body (the bytes after `<dir>`). Raises `ValueError` if too short.
    #[staticmethod]
    fn from_packed(#[pyo3(from_py_with = bytes_arg)] data: Vec<u8>) -> PyResult<Preset> {
        rec::Preset::from_packed(&data).map(Preset).ok_or_else(|| PyValueError::new_err("not a preset record"))
    }

    /// Decode a raw 110-byte record.
    #[staticmethod]
    fn from_raw(#[pyo3(from_py_with = bytes_arg)] data: Vec<u8>) -> PyResult<Preset> {
        rec::Preset::from_raw(&data).map(Preset).ok_or_else(|| PyValueError::new_err("need 110 bytes"))
    }

    /// The 110-byte record: the original bytes with every decoded field written back.
    fn to_raw<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        bytes(py, &self.0.to_raw())
    }

    /// Packed SysEx body for a `0x0B` write.
    fn to_packed<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        bytes(py, &self.0.to_packed())
    }

    /// Slot, 0-based (P01 = 0).
    #[getter]
    fn index(&self) -> u8 {
        self.0.index
    }

    #[setter]
    fn set_index(&mut self, i: u8) -> PyResult<()> {
        if i as usize >= rec::PRESET_COUNT {
            return Err(PyIndexError::new_err("preset index is 0..=98"));
        }
        self.0.index = i;
        Ok(())
    }

    #[getter]
    fn name(&self) -> &str {
        &self.0.name
    }

    /// Up to 16 printable ASCII characters are stored; others become `?`.
    #[setter]
    fn set_name(&mut self, n: String) {
        self.0.name = n;
    }

    #[getter]
    fn volume(&self) -> u8 {
        self.0.volume
    }

    #[setter]
    fn set_volume(&mut self, v: u8) {
        self.0.volume = v.min(100);
    }

    /// Chain order of the seven non-gate modules.
    #[getter]
    fn chain(&self) -> Vec<Module> {
        self.0.chain.iter().filter_map(|i| Module::from_index(*i)).collect()
    }

    #[setter]
    fn set_chain(&mut self, chain: Vec<Module>) -> PyResult<()> {
        let ids: Vec<u8> = chain.iter().map(|m| *m as u8).collect();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        if sorted != [0, 1, 2, 3, 4, 5, 7] {
            return Err(PyValueError::new_err("chain must order OD_FX, AMP, CAB, MOD, DELAY, REVERB and EQ, each once"));
        }
        self.0.chain.copy_from_slice(&ids);
        Ok(())
    }

    /// The eight effect modules' states, indexed by `Module.index` (copies).
    #[getter]
    fn modules(&self) -> Vec<ModuleState> {
        self.0.modules.iter().map(|s| ModuleState::from(*s)).collect()
    }

    #[setter]
    fn set_modules(&mut self, mods: Vec<ModuleState>) -> PyResult<()> {
        if mods.len() != 8 {
            return Err(PyValueError::new_err("need 8 module states"));
        }
        for (d, s) in self.0.modules.iter_mut().zip(&mods) {
            *d = s.into();
        }
        Ok(())
    }

    /// One module's state (a copy).
    fn module(&self, m: Module) -> PyResult<ModuleState> {
        Ok(self.0.modules[check_effect(m)?].into())
    }

    /// Change a module's on/off, model and/or parameter list in place.
    #[pyo3(signature = (module, *, on = None, model = None, params = None))]
    fn set_module(&mut self, module: Module, on: Option<bool>, model: Option<u8>, params: Option<Vec<u8>>) -> PyResult<()> {
        let st = &mut self.0.modules[check_effect(module)?];
        if let Some(on) = on {
            st.on = on;
        }
        if let Some(model) = model {
            st.model = model & 0x3F;
        }
        if let Some(p) = params {
            let mut ms = ModuleState::from(*st);
            ms.set_params(p)?;
            st.params = ms.params;
        }
        Ok(())
    }

    /// Set one parameter slot of a module in place.
    fn set_param(&mut self, module: Module, slot: usize, value: u8) -> PyResult<()> {
        let st = &mut self.0.modules[check_effect(module)?];
        *st.params.get_mut(slot).ok_or_else(|| PyIndexError::new_err("slot is 0..=11"))? = value;
        Ok(())
    }

    /// The raw 110-byte record as decoded (does not reflect later changes; see `to_raw`).
    #[getter]
    fn raw<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        bytes(py, &self.0.raw)
    }

    /// Same decoded sound (modules, volume, chain), ignoring parameter slots the models don't use.
    fn same_sound(&self, other: &Preset) -> bool {
        self.0.same_sound(&other.0)
    }

    fn copy(&self) -> Preset {
        self.clone()
    }

    fn __copy__(&self) -> Preset {
        self.clone()
    }

    fn __deepcopy__(&self, _memo: &Bound<'_, PyAny>) -> Preset {
        self.clone()
    }

    fn __eq__(&self, other: &Self) -> bool {
        self.0 == other.0
    }

    fn __repr__(&self) -> String {
        format!("Preset(index={}, name={:?}, volume={})", self.0.index, self.0.name, self.0.volume)
    }
}

/// Global system settings (SysEx `0x14`). Keeps the full record so unknown bytes survive.
#[pyclass(from_py_object, module = "open_stomp.records")]
#[derive(Clone)]
pub struct Settings(pub rec::Settings);

#[pymethods]
impl Settings {
    #[staticmethod]
    fn from_packed(#[pyo3(from_py_with = bytes_arg)] data: Vec<u8>) -> PyResult<Settings> {
        rec::Settings::from_packed(&data).map(Settings).ok_or_else(|| PyValueError::new_err("not a settings record"))
    }

    fn to_raw<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        bytes(py, &self.0.to_raw())
    }

    fn to_packed<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        bytes(py, &self.0.to_packed())
    }

    /// 0–20 = −10…+10 dB.
    #[getter]
    fn input_trim(&self) -> u8 {
        self.0.input_trim
    }

    #[setter]
    fn set_input_trim(&mut self, v: u8) {
        self.0.input_trim = v.min(20);
    }

    #[getter]
    fn input_trim_db(&self) -> i16 {
        self.0.input_trim_db()
    }

    #[getter]
    fn eq_bass(&self) -> u8 {
        self.0.eq_bass
    }

    #[setter]
    fn set_eq_bass(&mut self, v: u8) {
        self.0.eq_bass = v.min(100);
    }

    #[getter]
    fn eq_mid(&self) -> u8 {
        self.0.eq_mid
    }

    #[setter]
    fn set_eq_mid(&mut self, v: u8) {
        self.0.eq_mid = v.min(100);
    }

    #[getter]
    fn eq_treble(&self) -> u8 {
        self.0.eq_treble
    }

    #[setter]
    fn set_eq_treble(&mut self, v: u8) {
        self.0.eq_treble = v.min(100);
    }

    #[getter]
    fn wireless(&self) -> bool {
        self.0.wireless
    }

    #[setter]
    fn set_wireless(&mut self, v: bool) {
        self.0.wireless = v;
    }

    #[getter]
    fn loopback(&self) -> bool {
        self.0.loopback
    }

    #[setter]
    fn set_loopback(&mut self, v: bool) {
        self.0.loopback = v;
    }

    #[getter]
    fn raw<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        bytes(py, &self.0.raw)
    }

    fn copy(&self) -> Settings {
        self.clone()
    }

    fn __copy__(&self) -> Settings {
        self.clone()
    }

    fn __eq__(&self, other: &Self) -> bool {
        self.0 == other.0
    }

    fn __repr__(&self) -> String {
        let s = &self.0;
        format!(
            "Settings(input_trim_db={:+}, eq_bass={}, eq_mid={}, eq_treble={}, wireless={}, loopback={})",
            s.input_trim_db(),
            s.eq_bass,
            s.eq_mid,
            s.eq_treble,
            py_bool(s.wireless),
            py_bool(s.loopback)
        )
    }
}

/// `"P01"` … `"P99"` for a 0-based index.
#[pyfunction]
fn preset_label(index: u8) -> String {
    open_stomp::state::preset_label(index)
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<ModuleState>()?;
    m.add_class::<Preset>()?;
    m.add_class::<Settings>()?;
    m.add_function(wrap_pyfunction!(preset_label, m)?)?;
    m.add("PRESET_COUNT", rec::PRESET_COUNT)?;
    m.add("PRESET_LEN", rec::PRESET_LEN)?;
    m.add("DEFAULT_CHAIN", rec::DEFAULT_CHAIN.iter().filter_map(|i| Module::from_index(*i)).collect::<Vec<_>>())?;
    Ok(())
}
