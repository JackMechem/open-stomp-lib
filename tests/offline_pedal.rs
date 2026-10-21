//! The startup sync driven through [`RequestQueue`] against a fake pedal that answers every
//! request with the reply the real pedal gave in the main capture session.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use open_stomp::capture::{Entry, parse};
use open_stomp::queue::{Expect, RequestQueue, handshake};
use open_stomp::{DeviceState, Module, Msg, Origin};

fn main_session() -> Vec<Entry> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/captures/logs/01-main-session.log");
    parse(&std::fs::read_to_string(path).expect("capture log"))
}

/// A request without trailing zero padding (the official app pads some reads, e.g. the edit
/// buffer read carries 14 zero bytes; `req::edit_buffer()` carries none).
fn key(req: &[u8]) -> Vec<u8> {
    let body = &req[..req.len() - 1];
    let end = body.iter().rposition(|b| *b != 0).map_or(0, |i| i + 1);
    body[..end].to_vec()
}

/// Request (see [`key`]) → the first matching reply that followed it in the capture.
fn recorded_replies() -> HashMap<Vec<u8>, Vec<u8>> {
    let mut replies = HashMap::new();
    let mut waiting: Option<(Vec<u8>, Expect)> = None;
    for e in main_session() {
        let Entry::Message { origin, bytes, .. } = e else { continue };
        match origin {
            Origin::Host => {
                waiting = Expect::of(&bytes).filter(|_| !replies.contains_key(&key(&bytes))).map(|x| (key(&bytes), x));
            }
            Origin::Pedal => {
                if let Some((req, expect)) = &waiting
                    && expect.matches(&Msg::parse(&bytes))
                {
                    replies.insert(req.clone(), bytes);
                    waiting = None;
                }
            }
        }
    }
    replies
}

#[test]
fn sync_against_recorded_pedal() {
    let pedal = recorded_replies();
    let mut q = RequestQueue::new(Duration::from_secs(1));
    let mut st = DeviceState::default();
    let reqs = handshake();
    let total = reqs.len();
    q.start_sync(reqs);
    let mut now = Instant::now();
    let (mut sent, mut timeouts) = (0, 0);
    while q.progress() != (0, 0) || !q.is_empty() {
        if let Some(r) = q.pop() {
            st.apply(r.bytes(), Origin::Host);
            let reply = pedal.get(&key(r.bytes())).cloned();
            q.sent(r, now);
            sent += 1;
            match reply {
                Some(b) => {
                    assert!(q.on_message(&Msg::parse(&b)).unwrap().answered);
                    st.apply(&b, Origin::Pedal);
                }
                None => {
                    now += Duration::from_millis(1100);
                    assert!(!q.expire(now).unwrap().answered);
                    timeouts += 1;
                }
            }
        }
    }
    assert_eq!(sent, total);
    // the tuner state is never read during the captured session
    assert!(timeouts <= 1, "{timeouts} requests had no recorded reply");
    assert_eq!(st.firmware.as_deref(), Some("V1.1.1"));
    assert_eq!(st.product.as_deref(), Some("JME STOMPBOX"));
    assert!(st.presets.iter().all(|p| p.is_some()), "all 99 presets read");
    assert!(st.settings.is_some());
    // The capture starts with P81 "SV Bass" edited on the pedal, so the edit buffer matches no
    // stored preset by sound and the active slot stays unknown until the next preset change.
    assert!(st.live_known);
    assert_eq!(st.live.name, "SV Bass");
    assert_eq!(st.preset_name(80), Some("SV Bass"));
    assert!(st.current.is_none() && !st.stored(80).unwrap().same_sound(&st.live));
    assert!(st.live.modules[Module::Amp.index()].model < 40);
}
