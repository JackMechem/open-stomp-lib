//! Request pacing (docs/03): the pedal handles one SysEx request at a time, so requests and
//! writes are queued and each waits for its reply or ack before the next is sent, like the
//! official app. Live CCs/PCs carry no reply and need no queue.
//!
//! [`RequestQueue`] is the pure state machine (no I/O, time passed in) that
//! [`device`](crate::device) drives; use it directly to talk to the pedal over your own MIDI
//! transport:
//!
//! ```
//! use std::time::{Duration, Instant};
//! use open_stomp::queue::{RequestQueue, handshake};
//! use open_stomp::Msg;
//!
//! let mut q = RequestQueue::new(Duration::from_secs(1));
//! q.start_sync(handshake());
//! // loop: send what the queue releases ...
//! while let Some(req) = q.pop() {
//!     // out.send(req.bytes())?;
//!     q.sent(req, Instant::now());
//! }
//! // ... and feed it every complete message from the pedal
//! let reply = [0xF0, 0x00, 0x21, 0x7A, 0x10, 0x56, 0x31, 0xF7];
//! assert!(q.on_message(&Msg::parse(&reply)).is_some());
//! assert_eq!(q.progress(), (1, handshake().len()));
//! ```

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::midi::{Cmd, Msg, req};

/// What answers a request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Expect {
    /// Short frame reply of this kind with a payload (`0x10` version, `0x60` product).
    Short(u8),
    /// NEP reply (`dir` 2) to this cmd.
    Reply(u8),
    /// NEP write ack (`dir` 3) for this cmd.
    Ack(u8),
}

impl Expect {
    /// What the pedal sends back for an outgoing message, `None` for CCs/PCs (no reply).
    pub fn of(bytes: &[u8]) -> Option<Expect> {
        Some(match Msg::parse(bytes) {
            Msg::Short { kind: 0x00, .. } => Expect::Short(0x10),
            Msg::Short { kind, .. } => Expect::Short(kind),
            Msg::Nep { cmd, dir: 1, .. } => Expect::Ack(cmd),
            Msg::Nep { cmd, .. } => Expect::Reply(cmd),
            _ => return None,
        })
    }

    pub fn matches(self, msg: &Msg) -> bool {
        match (self, msg) {
            (Expect::Short(k), Msg::Short { kind, payload }) => *kind == k && !payload.is_empty(),
            (Expect::Reply(c), Msg::Nep { cmd, dir, .. }) => *cmd == c && *dir == 2,
            (Expect::Ack(c), Msg::Nep { cmd, dir, .. }) => *cmd == c && *dir == 3,
            _ => false,
        }
    }
}

/// One queued outgoing message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    bytes: Vec<u8>,
    expect: Option<Expect>,
    /// Part of the startup sync (counts towards its progress).
    sync: bool,
}

impl Request {
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// `None`: nothing comes back (CC/PC), done once sent.
    pub fn expect(&self) -> Option<Expect> {
        self.expect
    }

    pub fn is_sync(&self) -> bool {
        self.sync
    }
}

/// How an in-flight request ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Done {
    pub request: Request,
    /// The reply arrived (`false`: timed out).
    pub answered: bool,
}

/// Queue of SysEx requests and writes with at most one in flight.
#[derive(Debug)]
pub struct RequestQueue {
    queue: VecDeque<Request>,
    in_flight: Option<(Request, Instant)>,
    timeout: Duration,
    sync_total: usize,
    sync_done: usize,
}

/// The official app's startup reads (docs/03), all read-only. Presets come before the edit
/// buffer so the active preset can be identified by matching the edit buffer against them
/// (see [`DeviceState`](crate::DeviceState)).
pub fn handshake() -> Vec<Vec<u8>> {
    let mut reqs = vec![req::version(), req::product(), req::settings(), req::cur_state(), req::drum()];
    reqs.extend((0..10).map(req::ir_slot));
    reqs.extend((0..10).map(req::user_amp));
    reqs.extend((0..99).map(req::preset));
    reqs.extend([req::edit_buffer(), req::looper(), req::tuner()]);
    reqs
}

/// Follow-up reads after pedal-side changes that carry no data themselves: a preset saved on the
/// pedal (re-read it and the edit buffer) and the tuner toggled (read its state).
pub fn follow_up(msg: &Msg) -> Vec<Vec<u8>> {
    match msg {
        Msg::Notify { what, payload } if *what == Cmd::Preset as u8 => {
            let i = payload.first().copied().unwrap_or(0);
            vec![req::preset(i), req::edit_buffer()]
        }
        Msg::Notify { what, .. } if *what == Cmd::Tuner as u8 => vec![req::tuner()],
        _ => vec![],
    }
}

impl RequestQueue {
    /// `timeout`: how long to wait for a reply before moving on (the official app uses 1 s).
    pub fn new(timeout: Duration) -> Self {
        RequestQueue { queue: VecDeque::new(), in_flight: None, timeout, sync_total: 0, sync_done: 0 }
    }

    /// Drop everything queued and in flight, and any sync in progress.
    pub fn clear(&mut self) {
        self.queue.clear();
        self.in_flight = None;
        self.sync_total = 0;
        self.sync_done = 0;
    }

    /// Start a sync: replaces the queue with `reqs`, whose replies count towards [`progress`].
    ///
    /// [`progress`]: RequestQueue::progress
    pub fn start_sync(&mut self, reqs: Vec<Vec<u8>>) {
        self.clear();
        self.sync_total = reqs.len();
        for bytes in reqs {
            self.read(bytes);
        }
    }

    /// Queue a read. Ignored when the same request is already waiting.
    pub fn read(&mut self, bytes: Vec<u8>) {
        if !self.queue.iter().any(|r| r.bytes == bytes) {
            let expect = Expect::of(&bytes);
            self.queue.push_back(Request { bytes, expect, sync: self.sync_total > 0 });
        }
    }

    /// Queue a write (or any message that must keep its place after queued SysEx). Settings are
    /// rewritten whole on every change, so a queued settings write is replaced by the newer one.
    pub fn write(&mut self, bytes: Vec<u8>) {
        let expect = Expect::of(&bytes);
        if let Some(Expect::Ack(c)) = expect
            && c == Cmd::System as u8
            && let Some(r) = self.queue.iter_mut().find(|r| r.expect == expect)
        {
            r.bytes = bytes;
            return;
        }
        self.queue.push_back(Request { bytes, expect, sync: false });
    }

    /// The next message to send, if nothing is in flight. Pass it to [`sent`] once transmitted.
    ///
    /// [`sent`]: RequestQueue::sent
    pub fn pop(&mut self) -> Option<Request> {
        if self.in_flight.is_some() {
            return None;
        }
        self.queue.pop_front()
    }

    /// Record that `r` went out at `now`; it is in flight until answered or timed out.
    pub fn sent(&mut self, r: Request, now: Instant) {
        if r.expect.is_some() {
            self.in_flight = Some((r, now));
        }
    }

    /// Feed a complete incoming message. Returns the request it answers, if any.
    pub fn on_message(&mut self, msg: &Msg) -> Option<Done> {
        let (r, _) = self.in_flight.as_ref()?;
        if !r.expect.is_some_and(|e| e.matches(msg)) {
            return None;
        }
        let (request, _) = self.in_flight.take()?;
        self.finish(request, true)
    }

    /// Expire the in-flight request if it has waited longer than the timeout.
    pub fn expire(&mut self, now: Instant) -> Option<Done> {
        let (_, t) = self.in_flight.as_ref()?;
        if now.saturating_duration_since(*t) <= self.timeout {
            return None;
        }
        let (request, _) = self.in_flight.take()?;
        self.finish(request, false)
    }

    fn finish(&mut self, request: Request, answered: bool) -> Option<Done> {
        if request.sync && self.sync_total > 0 {
            self.sync_done += 1;
            if self.sync_done >= self.sync_total {
                self.sync_total = 0;
                self.sync_done = 0;
            }
        }
        Some(Done { request, answered })
    }

    /// `(done, total)` of the current sync; `(0, 0)` when none is running.
    pub fn progress(&self) -> (usize, usize) {
        (self.sync_done, self.sync_total)
    }

    pub fn in_flight(&self) -> Option<&Request> {
        self.in_flight.as_ref().map(|(r, _)| r)
    }

    /// Messages waiting (not counting the one in flight).
    pub fn len(&self) -> usize {
        self.queue.len()
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty() && self.in_flight.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::midi::host;
    use crate::records::Settings;

    fn t0() -> Instant {
        Instant::now()
    }

    #[test]
    fn one_in_flight_until_answered() {
        let mut q = RequestQueue::new(Duration::from_secs(1));
        q.read(req::settings());
        q.read(req::drum());
        q.read(req::settings()); // duplicate, ignored
        assert_eq!(q.len(), 2);
        let r = q.pop().unwrap();
        assert_eq!(r.expect(), Some(Expect::Reply(0x14)));
        q.sent(r, t0());
        assert!(q.pop().is_none());
        // unrelated traffic doesn't release it
        assert!(q.on_message(&Msg::parse(&[0xB0, 0x12, 5])).is_none());
        assert!(q.on_message(&Msg::parse(&req::settings())).is_none()); // dir 0, not a reply
        let done = q.on_message(&Msg::Nep { cmd: 0x14, dir: 2, payload: vec![0; 20] }).unwrap();
        assert!(done.answered);
        assert_eq!(q.pop().unwrap().bytes(), &req::drum()[..]);
    }

    #[test]
    fn live_messages_are_not_in_flight() {
        let mut q = RequestQueue::new(Duration::from_secs(1));
        q.write(host::program(3));
        q.write(host::cc(0x12, 50));
        let r = q.pop().unwrap();
        assert_eq!(r.expect(), None);
        q.sent(r, t0());
        assert!(q.in_flight().is_none());
        assert!(q.pop().is_some());
    }

    #[test]
    fn timeout_moves_on_and_counts_sync() {
        let mut q = RequestQueue::new(Duration::from_millis(100));
        q.start_sync(vec![req::version(), req::product()]);
        let start = t0();
        let r = q.pop().unwrap();
        assert_eq!(r.expect(), Some(Expect::Short(0x10)));
        q.sent(r, start);
        assert!(q.expire(start + Duration::from_millis(50)).is_none());
        let done = q.expire(start + Duration::from_millis(150)).unwrap();
        assert!(!done.answered && done.request.is_sync());
        assert_eq!(q.progress(), (1, 2));
        let r = q.pop().unwrap();
        q.sent(r, start);
        // product reply: F0 00 21 7A 60 <name> F7
        let reply = [0xF0, 0, 0x21, 0x7A, 0x60, b'J', b'M', b'E', 0xF7];
        assert!(q.on_message(&Msg::parse(&reply)).unwrap().answered);
        assert_eq!(q.progress(), (0, 0));
    }

    #[test]
    fn settings_writes_coalesce() {
        let mut q = RequestQueue::new(Duration::from_secs(1));
        q.write(host::program(1));
        let mut s = Settings { raw: vec![0; 17], ..Default::default() };
        q.write(host::settings_write(&s));
        s.input_trim = 13;
        q.write(host::settings_write(&s));
        assert_eq!(q.len(), 2);
        q.pop();
        assert_eq!(q.pop().unwrap().bytes(), &host::settings_write(&s)[..]);
    }

    #[test]
    fn handshake_matches_official_app() {
        let h = handshake();
        assert_eq!(h.len(), 5 + 10 + 10 + 99 + 3);
        assert!(h.iter().all(|r| Expect::of(r).is_some()), "every handshake read expects a reply");
    }

    #[test]
    fn follow_ups() {
        let saved = Msg::Notify { what: 0x0B, payload: vec![7] };
        assert_eq!(follow_up(&saved), vec![req::preset(7), req::edit_buffer()]);
        assert_eq!(follow_up(&Msg::Notify { what: 0x6F, payload: vec![] }), vec![req::tuner()]);
        assert!(follow_up(&Msg::Pc { ch: 0, prog: 1 }).is_empty());
    }
}
