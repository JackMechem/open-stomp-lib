//! `open_stomp.device`: the background engine (port scan, connect, sync, edits, replay).

use std::sync::mpsc::RecvTimeoutError;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use open_stomp::device as dev;
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyBytes;

use crate::catalog::Module;
use crate::records::{Preset, Settings};
use crate::state::Origin;
use crate::util::{bytes, bytes_arg, unix_seconds};

/// Longest stretch `recv` waits without checking for Ctrl-C (and without letting other
/// threads' commands through).
const SLICE: Duration = Duration::from_millis(100);

/// One report from the engine. `kind` says which fields are set:
///
/// | kind | fields |
/// |---|---|
/// | `"ports"` | `ports` (MIDI input port names) |
/// | `"status"` | `status` (`"disconnected"`, `"connected"`, `"replaying"`), `port`, `output`, `file`, `progress` |
/// | `"sync"` | `done`, `total` (`0, 0` = no sync running) |
/// | `"message"` | `at`, `origin`, `bytes` |
/// | `"mark"` | `at`, `text` (a `# MARK` line of a replayed capture) |
/// | `"info"` | `at`, `text`, `error` |
/// | `"reset"` | (a new session starts: reset your `DeviceState`) |
///
/// `at` is Unix time in seconds, like `time.time()`.
#[pyclass(frozen, get_all, module = "open_stomp.device")]
#[derive(Default)]
pub struct Update {
    kind: &'static str,
    ports: Option<Vec<String>>,
    status: Option<&'static str>,
    port: Option<String>,
    output: Option<bool>,
    file: Option<String>,
    progress: Option<f32>,
    done: Option<usize>,
    total: Option<usize>,
    at: Option<f64>,
    origin: Option<Origin>,
    bytes: Option<Py<PyBytes>>,
    text: Option<String>,
    error: Option<bool>,
}

impl Update {
    fn from(py: Python<'_>, u: dev::Update) -> Update {
        match u {
            dev::Update::Ports(p) => Update { kind: "ports", ports: Some(p), ..Default::default() },
            dev::Update::Status(s) => match s {
                dev::Status::Disconnected => Update { kind: "status", status: Some("disconnected"), ..Default::default() },
                dev::Status::Connected { port, output } => Update {
                    kind: "status",
                    status: Some("connected"),
                    port: Some(port),
                    output: Some(output),
                    ..Default::default()
                },
                dev::Status::Replaying { file, progress } => Update {
                    kind: "status",
                    status: Some("replaying"),
                    file: Some(file),
                    progress: Some(progress),
                    ..Default::default()
                },
            },
            dev::Update::Sync { done, total } => {
                Update { kind: "sync", done: Some(done), total: Some(total), ..Default::default() }
            }
            dev::Update::Message { at, origin, bytes: b } => Update {
                kind: "message",
                at: Some(unix_seconds(at)),
                origin: Some(origin.into()),
                bytes: Some(bytes(py, &b).unbind()),
                ..Default::default()
            },
            dev::Update::Mark { at, text } => {
                Update { kind: "mark", at: Some(unix_seconds(at)), text: Some(text), ..Default::default() }
            }
            dev::Update::Info { at, text, error } => Update {
                kind: "info",
                at: Some(unix_seconds(at)),
                text: Some(text),
                error: Some(error),
                ..Default::default()
            },
            dev::Update::Reset => Update { kind: "reset", ..Default::default() },
        }
    }
}

impl Update {
    pub fn kind(&self) -> &'static str {
        self.kind
    }

    pub fn message(&self) -> Option<(&Py<PyBytes>, Origin)> {
        Some((self.bytes.as_ref()?, self.origin?))
    }
}

#[pymethods]
impl Update {
    fn __repr__(&self) -> String {
        match self.kind {
            "ports" => format!("Update(kind='ports', ports={:?})", self.ports.as_deref().unwrap_or_default()),
            "status" => format!(
                "Update(kind='status', status={:?}, port={:?}, file={:?})",
                self.status.unwrap_or_default(),
                self.port,
                self.file
            ),
            "sync" => format!("Update(kind='sync', done={:?}, total={:?})", self.done, self.total),
            "message" => format!("Update(kind='message', origin={:?})", self.origin),
            "mark" | "info" => format!("Update(kind={:?}, text={:?})", self.kind, self.text),
            k => format!("Update(kind={k:?})"),
        }
    }
}

/// The background engine. Starts at once: finds the pedal, connects, syncs and reports
/// everything as `Update`s. Use as a context manager, or call `close()`, to stop it.
///
/// ```python
/// with Device() as dev:
///     for u in dev:            # blocks; Ctrl-C works
///         ...
/// ```
#[pyclass(module = "open_stomp.device")]
pub struct Device {
    inner: Mutex<Option<dev::Device>>,
}

fn closed() -> PyErr {
    PyRuntimeError::new_err("the device engine is closed")
}

impl Device {
    fn lock(&self) -> MutexGuard<'_, Option<dev::Device>> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn with(&self, f: impl FnOnce(&dev::Device)) -> PyResult<()> {
        let g = self.lock();
        let d = g.as_ref().ok_or_else(closed)?;
        f(d);
        Ok(())
    }
}

#[pymethods]
impl Device {
    /// Start the engine.
    ///
    /// * `client_name`: MIDI client name shown by the OS
    /// * `auto_connect`: connect to the first pedal-looking port, and again after replugging
    /// * `sync`: may send anything (startup reads and edits); `False` = listen only
    /// * `reply_timeout`: seconds a SysEx request waits for its reply
    /// * `scan_interval`: seconds between MIDI port rescans (hotplug)
    #[new]
    #[pyo3(signature = (*, client_name = "Open Stomp", auto_connect = true, sync = true, reply_timeout = 1.0, scan_interval = 1.5))]
    fn new(client_name: &str, auto_connect: bool, sync: bool, reply_timeout: f64, scan_interval: f64) -> PyResult<Self> {
        if !(reply_timeout > 0.0 && scan_interval > 0.0) {
            return Err(PyValueError::new_err("reply_timeout and scan_interval must be positive"));
        }
        let config = dev::Config {
            client_name: client_name.to_owned(),
            auto_connect,
            sync,
            reply_timeout: Duration::from_secs_f64(reply_timeout),
            scan_interval: Duration::from_secs_f64(scan_interval),
        };
        Ok(Device { inner: Mutex::new(Some(dev::Device::spawn(config))) })
    }

    /// Wait for the next update. `timeout` in seconds (`None` = forever); returns `None` on
    /// timeout. Releases the GIL while waiting and raises `KeyboardInterrupt` on Ctrl-C.
    #[pyo3(signature = (timeout = None))]
    fn recv(&self, py: Python<'_>, timeout: Option<f64>) -> PyResult<Option<Update>> {
        let deadline = timeout.map(|t| Instant::now() + Duration::from_secs_f64(t.max(0.0)));
        loop {
            let slice = match deadline {
                Some(d) => d.saturating_duration_since(Instant::now()).min(SLICE),
                None => SLICE,
            };
            let got = py.detach(|| {
                let g = self.lock();
                g.as_ref().map(|d| d.recv_timeout(slice))
            });
            match got {
                None => return Err(closed()),
                Some(Ok(u)) => return Ok(Some(Update::from(py, u))),
                Some(Err(RecvTimeoutError::Disconnected)) => return Err(PyRuntimeError::new_err("the device engine stopped")),
                Some(Err(RecvTimeoutError::Timeout)) => {}
            }
            py.check_signals()?;
            if deadline.is_some_and(|d| Instant::now() >= d) {
                return Ok(None);
            }
        }
    }

    /// The next update if one is waiting, else `None`.
    fn try_recv(&self, py: Python<'_>) -> PyResult<Option<Update>> {
        let g = self.lock();
        let d = g.as_ref().ok_or_else(closed)?;
        Ok(d.try_recv().ok().map(|u| Update::from(py, u)))
    }

    /// Every update waiting right now, without blocking.
    fn drain(&self, py: Python<'_>) -> PyResult<Vec<Update>> {
        let g = self.lock();
        let d = g.as_ref().ok_or_else(closed)?;
        Ok(d.drain().map(|u| Update::from(py, u)).collect())
    }

    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    /// Blocks for the next update; iteration ends when the engine is closed.
    fn __next__(&self, py: Python<'_>) -> PyResult<Option<Update>> {
        if self.lock().is_none() {
            return Ok(None);
        }
        match self.recv(py, None) {
            Err(e) if e.is_instance_of::<PyRuntimeError>(py) => Ok(None),
            r => r,
        }
    }

    /// Stop the engine, close the MIDI ports and join its thread. Safe to call twice.
    fn close(&self, py: Python<'_>) {
        let d = self.lock().take();
        py.detach(move || drop(d));
    }

    #[getter]
    fn closed(&self) -> bool {
        self.lock().is_none()
    }

    fn __enter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    #[pyo3(signature = (*_args))]
    fn __exit__(&self, py: Python<'_>, _args: &Bound<'_, pyo3::types::PyTuple>) -> bool {
        self.close(py);
        false
    }

    /// Connect to a MIDI input port by name (and the matching output); turns auto-connect on.
    fn connect(&self, port: String) -> PyResult<()> {
        self.with(|d| d.connect(port))
    }

    /// Close the connection and turn auto-connect off.
    fn disconnect(&self) -> PyResult<()> {
        self.with(|d| d.disconnect())
    }

    /// Read everything again (a `"reset"` update comes first).
    fn resync(&self) -> PyResult<()> {
        self.with(|d| d.resync())
    }

    /// Allow (`True`) or forbid (`False`, listen only) sending anything.
    fn set_sync(&self, on: bool) -> PyResult<()> {
        self.with(|d| d.set_sync(on))
    }

    /// Play a capture log instead of a pedal, at `speed`× its original timing.
    #[pyo3(signature = (path, speed = 4.0))]
    fn replay(&self, path: std::path::PathBuf, speed: f32) -> PyResult<()> {
        self.with(|d| d.replay(path, speed))
    }

    fn stop_replay(&self) -> PyResult<()> {
        self.with(|d| d.stop_replay())
    }

    /// Send a live message (CC/PC) now.
    fn send(&self, #[pyo3(from_py_with = bytes_arg)] data: Vec<u8>) -> PyResult<()> {
        self.with(|d| d.send(data))
    }

    /// Queue messages in order behind pending SysEx; writes wait for their ack.
    fn write(&self, messages: Vec<Bound<'_, PyAny>>) -> PyResult<()> {
        let msgs = messages.iter().map(bytes_arg).collect::<PyResult<Vec<_>>>()?;
        self.with(|d| d.write(msgs))
    }

    /// Select preset `index` (0-based, P01 = 0).
    fn select_preset(&self, index: u8) -> PyResult<()> {
        self.with(|d| d.select_preset(index))
    }

    /// Set a module's model and on/off together (they share one CC).
    fn set_head(&self, module: Module, model: u8, on: bool) -> PyResult<()> {
        self.with(|d| d.set_head(module.into(), model, on))
    }

    /// Set a module parameter by slot (the official app's display order).
    fn set_param(&self, module: Module, slot: usize, value: u8) -> PyResult<()> {
        self.with(|d| d.set_param(module.into(), slot, value))
    }

    /// Preset volume, 0–100.
    fn set_volume(&self, value: u8) -> PyResult<()> {
        self.with(|d| d.set_volume(value))
    }

    /// Make the pedal display jump to a module's page.
    fn show_page(&self, module: Module) -> PyResult<()> {
        self.with(|d| d.show_page(module.into()))
    }

    /// Press a looper button (`host.LOOP_*`).
    fn looper_button(&self, button: u8) -> PyResult<()> {
        self.with(|d| d.looper_button(button))
    }

    /// Store `preset` in slot `preset.index` with its name and select it (the official app's
    /// sequence). `source_slot` is the active preset the edits were made on.
    fn save_preset(&self, source_slot: u8, preset: PyRef<'_, Preset>) -> PyResult<()> {
        self.with(|d| d.save_preset(source_slot, &preset.0))
    }

    /// Import converted NAM data (2048 floats) into user AMP `slot` (0–9) like the official app:
    /// select the slot's amp model, write the slot-info record, then the data. Raises
    /// `ValueError` for a bad slot or data.
    #[pyo3(signature = (slot, name, data, factor = 1.0))]
    fn import_user_amp(&self, slot: u8, name: &str, data: Vec<f32>, factor: f32) -> PyResult<()> {
        let g = self.lock();
        let d = g.as_ref().ok_or_else(closed)?;
        d.import_user_amp(slot, name, &data, factor).map_err(crate::upload::err)
    }

    /// Delete user AMP `slot` and re-read its info.
    fn delete_user_amp(&self, slot: u8) -> PyResult<()> {
        let g = self.lock();
        let d = g.as_ref().ok_or_else(closed)?;
        d.delete_user_amp(slot).map_err(crate::upload::err)
    }

    /// Import IR data (2048 floats) into user IR `slot` (0–9) like the official app.
    fn import_user_ir(&self, slot: u8, name: &str, data: Vec<f32>) -> PyResult<()> {
        let g = self.lock();
        let d = g.as_ref().ok_or_else(closed)?;
        d.import_user_ir(slot, name, &data).map_err(crate::upload::err)
    }

    /// Write the global settings (start from `DeviceState.settings` so unknown bytes survive).
    fn write_settings(&self, settings: PyRef<'_, Settings>) -> PyResult<()> {
        self.with(|d| d.write_settings(&settings.0))
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        // joins the engine thread (<= one 50 ms tick)
        self.inner.get_mut().unwrap_or_else(|e| e.into_inner()).take();
    }
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Update>()?;
    m.add_class::<Device>()?;
    Ok(())
}
