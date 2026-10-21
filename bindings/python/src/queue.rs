//! `open_stomp.queue` and `open_stomp.capture`: request pacing and capture logs.

use std::time::{Duration, Instant};

use open_stomp::capture as cap;
use open_stomp::midi::Msg;
use open_stomp::queue as q;
use pyo3::prelude::*;
use pyo3::types::PyBytes;

use crate::state::Origin;
use crate::util::{bytes, bytes_arg, bytes_list};

fn expect_str(e: Option<q::Expect>) -> Option<String> {
    e.map(|e| match e {
        q::Expect::Short(k) => format!("short 0x{k:02X}"),
        q::Expect::Reply(c) => format!("reply 0x{c:02X}"),
        q::Expect::Ack(c) => format!("ack 0x{c:02X}"),
    })
}

/// One queued outgoing message, as released by `RequestQueue.pop`.
#[pyclass(frozen, module = "open_stomp.queue")]
pub struct Request(q::Request);

#[pymethods]
impl Request {
    #[getter]
    fn bytes<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        bytes(py, self.0.bytes())
    }

    /// What answers it, e.g. `"reply 0x0B"`, `"ack 0x14"`, `"short 0x10"`; `None` for CC/PC.
    #[getter]
    fn expect(&self) -> Option<String> {
        expect_str(self.0.expect())
    }

    /// Part of the startup sync (counts towards `RequestQueue.progress`).
    #[getter]
    fn is_sync(&self) -> bool {
        self.0.is_sync()
    }

    fn __repr__(&self) -> String {
        let b = self.0.bytes();
        format!("Request({}{}, expect={:?})", open_stomp::midi::hex(&b[..b.len().min(8)]), if b.len() > 8 { " …" } else { "" }, self.expect())
    }
}

/// How an in-flight request ended.
#[pyclass(frozen, get_all, module = "open_stomp.queue")]
pub struct Done {
    request: Py<Request>,
    /// The reply arrived (`False`: timed out).
    answered: bool,
}

/// Queue of SysEx requests and writes with at most one in flight (docs/03).
///
/// Times are seconds since the queue was created (pass `at=None` for "now"), so tests can
/// drive timeouts without sleeping.
#[pyclass(module = "open_stomp.queue")]
pub struct RequestQueue {
    inner: q::RequestQueue,
    epoch: Instant,
}

impl RequestQueue {
    fn instant(&self, at: Option<f64>) -> Instant {
        match at {
            Some(s) => self.epoch + Duration::from_secs_f64(s.max(0.0)),
            None => Instant::now(),
        }
    }

    fn done(py: Python<'_>, d: Option<q::Done>) -> PyResult<Option<Done>> {
        d.map(|d| Ok(Done { request: Py::new(py, Request(d.request))?, answered: d.answered })).transpose()
    }
}

#[pymethods]
impl RequestQueue {
    /// `timeout`: seconds to wait for a reply before moving on (the official app uses 1 s).
    #[new]
    #[pyo3(signature = (timeout = 1.0))]
    fn new(timeout: f64) -> Self {
        RequestQueue { inner: q::RequestQueue::new(Duration::from_secs_f64(timeout.max(0.0))), epoch: Instant::now() }
    }

    /// Drop everything queued and in flight, and any sync in progress.
    fn clear(&mut self) {
        self.inner.clear();
    }

    /// Replace the queue with `requests`, whose replies count towards `progress`.
    fn start_sync(&mut self, requests: Vec<Bound<'_, PyAny>>) -> PyResult<()> {
        let reqs = requests.iter().map(bytes_arg).collect::<PyResult<Vec<_>>>()?;
        self.inner.start_sync(reqs);
        Ok(())
    }

    /// Queue a read (ignored when the same request is already waiting).
    fn read(&mut self, #[pyo3(from_py_with = bytes_arg)] data: Vec<u8>) {
        self.inner.read(data);
    }

    /// Queue a write; a queued settings write is replaced by a newer one.
    fn write(&mut self, #[pyo3(from_py_with = bytes_arg)] data: Vec<u8>) {
        self.inner.write(data);
    }

    /// The next message to send, if nothing is in flight. Pass it to `sent` once transmitted.
    fn pop(&mut self) -> Option<Request> {
        self.inner.pop().map(Request)
    }

    /// Record that `request` went out (at `at` seconds, default now).
    #[pyo3(signature = (request, at = None))]
    fn sent(&mut self, request: PyRef<'_, Request>, at: Option<f64>) {
        let t = self.instant(at);
        self.inner.sent(request.0.clone(), t);
    }

    /// Feed a complete incoming message; returns the request it answered, if any.
    fn on_message(&mut self, py: Python<'_>, #[pyo3(from_py_with = bytes_arg)] data: Vec<u8>) -> PyResult<Option<Done>> {
        let d = self.inner.on_message(&Msg::parse(&data));
        Self::done(py, d)
    }

    /// Expire the in-flight request if it waited longer than the timeout (at `at`, default now).
    #[pyo3(signature = (at = None))]
    fn expire(&mut self, py: Python<'_>, at: Option<f64>) -> PyResult<Option<Done>> {
        let t = self.instant(at);
        let d = self.inner.expire(t);
        Self::done(py, d)
    }

    /// `(done, total)` of the current sync; `(0, 0)` when none is running.
    #[getter]
    fn progress(&self) -> (usize, usize) {
        self.inner.progress()
    }

    #[getter]
    fn in_flight(&self) -> Option<Request> {
        self.inner.in_flight().cloned().map(Request)
    }

    /// Nothing queued and nothing in flight.
    #[getter]
    fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Messages waiting (not counting the one in flight).
    fn __len__(&self) -> usize {
        self.inner.len()
    }
}

/// The official app's startup reads (127 requests).
#[pyfunction]
fn handshake<'py>(py: Python<'py>) -> Vec<Bound<'py, PyBytes>> {
    bytes_list(py, &q::handshake())
}

/// Reads to queue after a pedal notification that carries no data (preset saved, tuner toggled).
#[pyfunction]
fn follow_up<'py>(py: Python<'py>, #[pyo3(from_py_with = bytes_arg)] data: Vec<u8>) -> Vec<Bound<'py, PyBytes>> {
    bytes_list(py, &q::follow_up(&Msg::parse(&data)))
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Request>()?;
    m.add_class::<Done>()?;
    m.add_class::<RequestQueue>()?;
    m.add_function(wrap_pyfunction!(handshake, m)?)?;
    m.add_function(wrap_pyfunction!(follow_up, m)?)?;
    Ok(())
}

// ───────────────────────────── capture ─────────────────────────────

/// One entry of a capture log: a `"message"` or a `"mark"` annotation.
#[pyclass(frozen, module = "open_stomp.capture")]
pub struct Entry(cap::Entry);

#[pymethods]
impl Entry {
    /// `"message"` or `"mark"`.
    #[getter]
    fn kind(&self) -> &'static str {
        match self.0 {
            cap::Entry::Mark { .. } => "mark",
            cap::Entry::Message { .. } => "message",
        }
    }

    /// Unix time in milliseconds.
    #[getter]
    fn ms(&self) -> u64 {
        match self.0 {
            cap::Entry::Mark { ms, .. } | cap::Entry::Message { ms, .. } => ms,
        }
    }

    /// The annotation of a mark.
    #[getter]
    fn text(&self) -> Option<&str> {
        match &self.0 {
            cap::Entry::Mark { text, .. } => Some(text),
            _ => None,
        }
    }

    #[getter]
    fn origin(&self) -> Option<Origin> {
        match self.0 {
            cap::Entry::Message { origin, .. } => Some(origin.into()),
            _ => None,
        }
    }

    /// The complete message of a `"message"` entry.
    #[getter]
    fn bytes<'py>(&self, py: Python<'py>) -> Option<Bound<'py, PyBytes>> {
        match &self.0 {
            cap::Entry::Message { bytes: b, .. } => Some(bytes(py, b)),
            _ => None,
        }
    }

    fn __repr__(&self) -> String {
        match &self.0 {
            cap::Entry::Mark { ms, text } => format!("Entry(kind='mark', ms={ms}, text={text:?})"),
            cap::Entry::Message { ms, origin, bytes } => {
                format!("Entry(kind='message', ms={ms}, origin={origin:?}, bytes={})", open_stomp::midi::hex(bytes))
            }
        }
    }
}

/// Parse a whole capture log (docs/07) into complete messages and marks.
#[pyfunction]
fn parse(text: &str) -> Vec<Entry> {
    cap::parse(text).into_iter().map(Entry).collect()
}

pub fn register_capture(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Entry>()?;
    m.add_function(wrap_pyfunction!(parse, m)?)?;
    Ok(())
}
