//! Python bindings for `open-stomp` (`import open_stomp`).
//!
//! The extension is `open_stomp._native`; `python/open_stomp/__init__.py` exposes its
//! submodules as `open_stomp.midi`, `open_stomp.catalog`, … mirroring the Rust crate.

mod catalog;
mod device;
mod midi;
mod queue;
mod records;
mod state;
mod upload;
mod util;

use pyo3::prelude::*;

fn submodule<'py>(
    parent: &Bound<'py, PyModule>,
    name: &str,
    register: impl FnOnce(&Bound<'py, PyModule>) -> PyResult<()>,
) -> PyResult<()> {
    let m = PyModule::new(parent.py(), name)?;
    register(&m)?;
    parent.add_submodule(&m)
}

/// Whether a MIDI port name belongs to the pedal ("JME", "SORAN" or "STOMPBOX", any case).
#[pyfunction]
fn is_stomp_port(name: &str) -> bool {
    open_stomp::is_stomp_port(name)
}

#[pymodule]
fn _native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    let py = m.py();
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    m.add("USB_ID", open_stomp::USB_ID)?;
    m.add_function(wrap_pyfunction!(is_stomp_port, m)?)?;
    submodule(m, "pack", midi::register_pack)?;
    submodule(m, "midi", |md| midi::register(py, md))?;
    submodule(m, "catalog", catalog::register)?;
    submodule(m, "records", records::register)?;
    submodule(m, "state", state::register)?;
    submodule(m, "queue", queue::register)?;
    submodule(m, "capture", queue::register_capture)?;
    submodule(m, "upload", upload::register)?;
    submodule(m, "device", device::register)?;
    Ok(())
}
