pub mod addr;
pub mod invoke_id;
pub mod segmentation;
pub mod slot;
pub mod types;

use std::collections::HashMap;

use crate::codec::apdu::{
    decode_abort, decode_complex_ack, decode_error_apdu, decode_reject, decode_segment_ack,
    decode_simple_ack, encode_confirmed_request, ConfirmedRequestParams, MaxApduLength,
    MaxSegments,
};
use crate::codec::bvlc::{self, BvlcFunction};
use crate::codec::npdu;
use crate::services::i_am::{
    decode_i_am, decode_i_am_router_to_network, I_AM_SERVICE_CHOICE, MSG_I_AM_ROUTER_TO_NETWORK,
};

use addr::BacnetAddr;
use invoke_id::InvokeIdPool;
use segmentation::{RecvAction, RecvSegState, SendSegState};
use slot::InFlightSlot;
use types::{BacnetEvent, BacnetService, Input, Output, StackConfig, UnconfirmedMessage};

// ─────────────────────────────────────────────────────────────────────────────
// Stack
// ─────────────────────────────────────────────────────────────────────────────

/// The sans-IO BACnet/IP client stack.
///
/// # Sans-IO design
///
/// `Stack` is a pure state machine — it never touches a socket, spawns a
/// thread, or reads a clock.  All external inputs arrive through
/// [`Stack::process`] and all side-effects are returned as a `Vec<Output>`.
/// The host (Python asyncio, a test harness, or any other runtime) is
/// responsible for:
///
/// * Calling `process(Input::Received { data, src })` for every incoming UDP
///   datagram.
/// * Calling `process(Input::Tick { now })` whenever the timer fires. The
///   stack returns `Output::Deadline(t)` to tell the host when to fire next.
/// * Calling `process(Input::Send { service, dest })` to enqueue a new
///   confirmed request.
/// * Forwarding each `Output::Transmit { data, dest }` to the UDP socket.
/// * Delivering each `Output::Event(e)` to the application.
///
/// # Example (Rust)
///
/// ```rust,no_run
/// use libbacnet::stack::{Stack, types::{Input, Output, BacnetService}};
/// use libbacnet::stack::addr::BacnetAddr;
/// use libbacnet::codec::types::ObjectIdentifier;
/// use libbacnet::enums::PropertyIdentifier;
/// use std::net::Ipv4Addr;
///
/// let mut stack = Stack::new(Default::default());
/// let dest = BacnetAddr { addr: Ipv4Addr::new(192, 168, 1, 10), port: 47808 };
/// let outputs = stack.process(Input::Send {
///     service: BacnetService::ReadProperty {
///         object_id: ObjectIdentifier::new(
///             libbacnet::codec::types::ObjectType::Device, 42,
///         ),
///         property_id: PropertyIdentifier::PresentValue,
///         array_index: None,
///     },
///     dest,
/// });
/// // outputs contains Output::Transmit { data, dest } and Output::Deadline(t).
/// ```
pub struct Stack {
    config: StackConfig,
    /// Per-destination invoke ID pools.
    invoke_id_pools: HashMap<BacnetAddr, InvokeIdPool>,
    /// In-flight confirmed requests keyed by (dest, invoke_id).
    in_flight: HashMap<(BacnetAddr, u8), InFlightSlot>,
    /// Send-side segmentation state keyed by (dest, invoke_id).
    send_seg: HashMap<(BacnetAddr, u8), SendSegState>,
    /// Receive-side reassembly buffers keyed by (src, invoke_id).
    recv_seg: HashMap<(BacnetAddr, u8), RecvSegState>,
}

impl Stack {
    /// Create a new stack with the given configuration.
    ///
    /// # Example
    ///
    /// ```rust
    /// use libbacnet::stack::{Stack, types::StackConfig};
    ///
    /// // Default configuration (3 s timeout, 3 retries, 1476 B max APDU, 2 MB segment buffer).
    /// let stack = Stack::new(StackConfig::default());
    ///
    /// // Custom configuration.
    /// let cfg = StackConfig {
    ///     apdu_timeout_secs: 5.0,
    ///     apdu_retries: 2,
    ///     max_apdu_length: 480,
    ///     max_segment_buffer: 512 * 1024,
    /// };
    /// let stack = Stack::new(cfg);
    /// ```
    pub fn new(config: StackConfig) -> Self {
        Self {
            config,
            invoke_id_pools: HashMap::new(),
            in_flight: HashMap::new(),
            send_seg: HashMap::new(),
            recv_seg: HashMap::new(),
        }
    }

    /// Process one input event and return zero or more outputs.
    ///
    /// This is the **single entry point** for all protocol state changes.
    /// Call it once per event and act on every item in the returned `Vec`.
    ///
    /// # Inputs and their outputs
    ///
    /// | Input variant | Typical outputs |
    /// |---|---|
    /// | `Received { data, src }` | `Event(Response \| Timeout \| Abort \| Error \| UnconfirmedReceived)`, optionally `Transmit` (SegACK) |
    /// | `Tick { now }` | `Transmit` (retransmit), `Event(Timeout)`, `Deadline` |
    /// | `Send { service, dest }` | `Transmit` (request frame(s)), `Deadline` |
    ///
    /// # Panics
    ///
    /// Does not panic; malformed inputs produce `Event(Error)` outputs instead.
    pub fn process(&mut self, input: Input) -> Vec<Output> {
        match input {
            Input::Received { data, src } => self.handle_received(&data, src),
            Input::Tick { now } => self.handle_tick(now),
            Input::Send { service, dest } => self.handle_send(service, dest),
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // handle_send
    // ─────────────────────────────────────────────────────────────────────────

    fn handle_send(&mut self, service: BacnetService, dest: BacnetAddr) -> Vec<Output> {
        // Allocate an invoke ID for this destination.
        let pool = self.invoke_id_pools.entry(dest).or_default();

        let invoke_id = match pool.allocate() {
            Some(id) => id,
            None => {
                return vec![Output::Event(BacnetEvent::Error {
                    invoke_id: 0,
                    message: "invoke ID pool exhausted".into(),
                })];
            }
        };

        let service_choice = service.choice();
        let service_data = service.encode();

        // Overhead of an unsegmented ConfirmedRequest APDU header: 4 bytes
        // (byte0, byte1, invoke_id, service_choice). Use service_data.len() to
        // decide whether segmentation is needed.
        let unseg_apdu_len = 4 + service_data.len();

        if unseg_apdu_len > self.config.max_apdu_length {
            // ── Segmented send path ────────────────────────────
            // Fragment size = max_apdu_length minus the segmented header (6 bytes:
            // byte0, byte1, invoke_id, seq#, window, service_choice).
            let fragment_size = self.config.max_apdu_length.saturating_sub(6);
            let now = 0.0_f64;
            let (seg_state, frames) = SendSegState::new(
                service_data,
                service_choice,
                invoke_id,
                dest,
                fragment_size,
                now,
            );
            self.send_seg.insert((dest, invoke_id), seg_state);

            // Store a placeholder in-flight slot so the invoke ID stays allocated.
            // request_bytes is empty — retransmit is handled via send_seg.
            self.in_flight.insert(
                (dest, invoke_id),
                InFlightSlot {
                    request_bytes: vec![],
                    dest,
                    sent_at: now,
                    retry_count: 0,
                    next_retry_at: f64::INFINITY, // managed by seg timeout
                },
            );

            let deadline = now + segmentation::SEG_TIMEOUT_SECS;
            let mut outputs: Vec<Output> = frames
                .into_iter()
                .map(|f| Output::Transmit { data: f, dest })
                .collect();
            outputs.push(Output::Deadline(deadline));
            return outputs;
        }

        // ── Unsegmented send path ────────────────────────────────────
        let apdu = encode_confirmed_request(&ConfirmedRequestParams {
            segmented_response_accepted: true,
            max_segments: MaxSegments::Unspecified,
            max_apdu_length: MaxApduLength::Up1476,
            invoke_id,
            segmentation: None,
            service_choice,
            service_data: &service_data,
        });

        let npdu_bytes = npdu::encode(&npdu::NpduEncodeParams {
            apdu: &apdu,
            data_expecting_reply: true,
            ..Default::default()
        });
        let frame = bvlc::encode(BvlcFunction::OriginalUnicastNpdu, &npdu_bytes);

        let now = 0.0_f64;
        let next_retry_at = now + self.config.apdu_timeout_secs;

        let slot = InFlightSlot {
            request_bytes: frame.clone(),
            dest,
            sent_at: now,
            retry_count: 0,
            next_retry_at,
        };
        self.in_flight.insert((dest, invoke_id), slot);

        vec![
            Output::Transmit { data: frame, dest },
            Output::Deadline(next_retry_at),
        ]
    }

    // ─────────────────────────────────────────────────────────────────────────
    // handle_received
    // ─────────────────────────────────────────────────────────────────────────

    fn handle_received(&mut self, data: &[u8], src: BacnetAddr) -> Vec<Output> {
        let bvlc_frame = match bvlc::decode(data) {
            Ok(f) => f,
            Err(_) => return vec![],
        };

        let npdu_frame = match npdu::decode(bvlc_frame.npdu) {
            Ok(f) => f,
            Err(_) => return vec![],
        };

        if npdu_frame.is_network_layer_message {
            return self.handle_network_layer_message(npdu_frame.apdu, src);
        }

        let apdu = npdu_frame.apdu;
        if apdu.is_empty() {
            return vec![];
        }

        let pdu_type = apdu[0] >> 4;

        match pdu_type {
            // ComplexACK (3) — may be segmented
            3 => {
                let ack = match decode_complex_ack(apdu) {
                    Ok(a) => a,
                    Err(_) => return vec![],
                };
                let invoke_id = ack.invoke_id;

                if let Some(seg) = ack.segmentation {
                    // Segmented response fragment
                    return self.handle_recv_fragment(
                        src,
                        invoke_id,
                        seg.sequence_number,
                        seg.more_follows,
                        seg.proposed_window_size,
                        ack.service_data,
                        0.0,
                    );
                }

                // Unsegmented ComplexACK
                self.free_slot(src, invoke_id);
                vec![Output::Event(BacnetEvent::Response {
                    invoke_id,
                    payload: ack.service_data.to_vec(),
                })]
            }
            // SimpleACK (2)
            2 => {
                let ack = match decode_simple_ack(apdu) {
                    Ok(a) => a,
                    Err(_) => return vec![],
                };
                let invoke_id = ack.invoke_id;
                self.free_slot(src, invoke_id);
                vec![Output::Event(BacnetEvent::Response {
                    invoke_id,
                    payload: vec![],
                })]
            }
            // SegmentACK (4)
            4 => {
                let seg_ack = match decode_segment_ack(apdu) {
                    Ok(a) => a,
                    Err(_) => return vec![],
                };
                self.handle_seg_ack(
                    src,
                    seg_ack.invoke_id,
                    seg_ack.sequence_number,
                    seg_ack.actual_window_size,
                    0.0,
                )
            }
            // Error (5)
            5 => {
                let err = match decode_error_apdu(apdu) {
                    Ok(e) => e,
                    Err(_) => return vec![],
                };
                let invoke_id = err.invoke_id;
                self.free_slot(src, invoke_id);
                vec![Output::Event(BacnetEvent::Error {
                    invoke_id,
                    message: format!("error class={} code={}", err.error_class, err.error_code),
                })]
            }
            // Abort (7)
            7 => {
                let abort = match decode_abort(apdu) {
                    Ok(a) => a,
                    Err(_) => return vec![],
                };
                let invoke_id = abort.invoke_id;
                self.free_slot(src, invoke_id);
                vec![Output::Event(BacnetEvent::Abort {
                    invoke_id,
                    reason: abort.abort_reason,
                })]
            }
            // Reject (6)
            6 => {
                let rej = match decode_reject(apdu) {
                    Ok(r) => r,
                    Err(_) => return vec![],
                };
                let invoke_id = rej.invoke_id;
                self.free_slot(src, invoke_id);
                vec![Output::Event(BacnetEvent::Error {
                    invoke_id,
                    message: format!("reject reason={}", rej.reject_reason),
                })]
            }
            // UnconfirmedRequest (1)
            1 => {
                if apdu.len() < 2 {
                    return vec![];
                }
                let service_choice = apdu[1];
                let service_data = &apdu[2..];
                self.handle_unconfirmed(service_choice, service_data, src)
            }
            _ => vec![],
        }
    }

    /// Handle a segmented ComplexACK fragment.
    #[allow(clippy::too_many_arguments)]
    fn handle_recv_fragment(
        &mut self,
        src: BacnetAddr,
        invoke_id: u8,
        sequence_number: u8,
        more_follows: bool,
        proposed_window_size: u8,
        service_data: &[u8],
        now: f64,
    ) -> Vec<Output> {
        let max_buffer = self.config.max_segment_buffer;
        let state = self
            .recv_seg
            .entry((src, invoke_id))
            .or_insert_with(|| RecvSegState::new(proposed_window_size, max_buffer, now));

        let action = state.accept_fragment(
            sequence_number,
            more_follows,
            proposed_window_size,
            service_data.to_vec(),
            src,
            invoke_id,
            now,
        );

        match action {
            RecvAction::Continue => vec![],
            RecvAction::SendSegAck(frame) => {
                vec![Output::Transmit {
                    data: frame,
                    dest: src,
                }]
            }
            RecvAction::Complete {
                seg_ack_frame,
                payload,
            } => {
                self.recv_seg.remove(&(src, invoke_id));
                self.free_slot(src, invoke_id);
                vec![
                    Output::Transmit {
                        data: seg_ack_frame,
                        dest: src,
                    },
                    Output::Event(BacnetEvent::Response { invoke_id, payload }),
                ]
            }
            RecvAction::Abort(reason) => {
                self.recv_seg.remove(&(src, invoke_id));
                self.free_slot(src, invoke_id);
                vec![Output::Event(BacnetEvent::Abort { invoke_id, reason })]
            }
        }
    }

    /// Handle a received SegACK for an outgoing segmented request.
    fn handle_seg_ack(
        &mut self,
        _src: BacnetAddr,
        invoke_id: u8,
        sequence_number: u8,
        actual_window_size: u8,
        now: f64,
    ) -> Vec<Output> {
        let dest = match self.in_flight.get(&(_src, invoke_id)) {
            Some(slot) => slot.dest,
            None => return vec![],
        };

        if let Some(seg_state) = self.send_seg.get_mut(&(dest, invoke_id)) {
            match seg_state.handle_seg_ack(sequence_number, actual_window_size, now) {
                None => {
                    // All segments acknowledged — complete.
                    self.send_seg.remove(&(dest, invoke_id));
                    self.free_slot(dest, invoke_id);
                    vec![]
                }
                Some(frames) => frames
                    .into_iter()
                    .map(|f| Output::Transmit { data: f, dest })
                    .collect(),
            }
        } else {
            vec![]
        }
    }

    fn handle_network_layer_message(&mut self, msg: &[u8], src: BacnetAddr) -> Vec<Output> {
        if msg.is_empty() {
            return vec![];
        }
        if msg[0] == MSG_I_AM_ROUTER_TO_NETWORK {
            if let Ok(m) = decode_i_am_router_to_network(msg) {
                return vec![Output::Event(BacnetEvent::UnconfirmedReceived {
                    src,
                    message: UnconfirmedMessage::IAmRouterToNetwork {
                        networks: m.networks,
                    },
                })];
            }
        }
        vec![]
    }

    fn handle_unconfirmed(
        &mut self,
        service_choice: u8,
        service_data: &[u8],
        src: BacnetAddr,
    ) -> Vec<Output> {
        if service_choice == I_AM_SERVICE_CHOICE {
            if let Ok(msg) = decode_i_am(service_data) {
                return vec![Output::Event(BacnetEvent::UnconfirmedReceived {
                    src,
                    message: UnconfirmedMessage::IAm {
                        device_id: msg.device_id,
                        max_apdu: msg.max_apdu_length_accepted,
                        segmentation: msg.segmentation_supported as u8,
                        vendor_id: msg.vendor_id,
                    },
                })];
            }
        }
        vec![]
    }

    // ─────────────────────────────────────────────────────────────────────────
    // handle_tick
    // ─────────────────────────────────────────────────────────────────────────

    fn handle_tick(&mut self, now: f64) -> Vec<Output> {
        let mut outputs: Vec<Output> = Vec::new();
        let mut to_free: Vec<(BacnetAddr, u8)> = Vec::new();
        let mut earliest_deadline: Option<f64> = None;

        // ── Unsegmented retry / timeout ────────────────────────────────
        for (&(dest, invoke_id), slot) in &mut self.in_flight {
            // Skip segmented slots (they're managed separately).
            if slot.request_bytes.is_empty() {
                continue;
            }
            if now >= slot.next_retry_at {
                if slot.retry_count >= self.config.apdu_retries {
                    outputs.push(Output::Event(BacnetEvent::Timeout { invoke_id }));
                    to_free.push((dest, invoke_id));
                } else {
                    slot.retry_count += 1;
                    slot.next_retry_at = now + self.config.apdu_timeout_secs;
                    outputs.push(Output::Transmit {
                        data: slot.request_bytes.clone(),
                        dest,
                    });
                    let d = slot.next_retry_at;
                    earliest_deadline = Some(earliest_deadline.map_or(d, |e: f64| e.min(d)));
                }
            } else {
                let d = slot.next_retry_at;
                earliest_deadline = Some(earliest_deadline.map_or(d, |e: f64| e.min(d)));
            }
        }

        for key in to_free.drain(..) {
            self.free_slot(key.0, key.1);
        }

        // ── Send-side segmentation window timeout ───────────────────────
        let seg_keys: Vec<(BacnetAddr, u8)> = self.send_seg.keys().copied().collect();
        for key in seg_keys {
            if let Some(seg_state) = self.send_seg.get_mut(&key) {
                if seg_state.is_window_timed_out(now) {
                    let frames = seg_state.retransmit_window(now);
                    let dest = key.0;
                    for f in frames {
                        outputs.push(Output::Transmit { data: f, dest });
                    }
                    let d = now + segmentation::SEG_TIMEOUT_SECS;
                    earliest_deadline = Some(earliest_deadline.map_or(d, |e: f64| e.min(d)));
                } else {
                    let d = seg_state.window_sent_at + segmentation::SEG_TIMEOUT_SECS;
                    earliest_deadline = Some(earliest_deadline.map_or(d, |e: f64| e.min(d)));
                }
            }
        }

        // ── Receive-side reassembly timeout ─────────────────────────────
        let recv_keys: Vec<(BacnetAddr, u8)> = self.recv_seg.keys().copied().collect();
        for key in recv_keys {
            if let Some(state) = self.recv_seg.get(&key) {
                if state.is_timed_out(now) {
                    let invoke_id = key.1;
                    outputs.push(Output::Event(BacnetEvent::Abort {
                        invoke_id,
                        reason: segmentation::ABORT_REASON_REASSEMBLY_TIMEOUT,
                    }));
                    self.recv_seg.remove(&key);
                    self.free_slot(key.0, invoke_id);
                }
            }
        }

        if let Some(d) = earliest_deadline {
            outputs.push(Output::Deadline(d));
        }

        outputs
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Helpers
    // ─────────────────────────────────────────────────────────────────────────

    fn free_slot(&mut self, dest: BacnetAddr, invoke_id: u8) {
        self.in_flight.remove(&(dest, invoke_id));
        if let Some(pool) = self.invoke_id_pools.get_mut(&dest) {
            pool.free(invoke_id);
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::types::ObjectIdentifier;
    use crate::codec::types::ObjectType;
    use crate::enums::PropertyIdentifier;

    fn default_stack() -> Stack {
        Stack::new(StackConfig::default())
    }

    fn dest() -> BacnetAddr {
        BacnetAddr::new([192, 168, 1, 1], 47808)
    }

    fn read_property_service() -> BacnetService {
        BacnetService::ReadProperty {
            object_id: ObjectIdentifier::new(ObjectType::Device, 1),
            property_id: PropertyIdentifier::PresentValue,
            array_index: None,
        }
    }

    /// Helper: wrap APDU bytes in NPDU + BVLC to simulate a received frame.
    fn make_received_frame(apdu: &[u8]) -> Vec<u8> {
        let npdu_bytes = npdu::encode(&npdu::NpduEncodeParams {
            apdu,
            ..Default::default()
        });
        bvlc::encode(BvlcFunction::OriginalUnicastNpdu, &npdu_bytes)
    }

    // ── Send ────────────────────────────────────────────────────────────────

    #[test]
    fn test_send_returns_transmit_and_deadline() {
        let mut stack = default_stack();
        let outputs = stack.process(Input::Send {
            service: read_property_service(),
            dest: dest(),
        });
        let has_transmit = outputs.iter().any(|o| matches!(o, Output::Transmit { .. }));
        let has_deadline = outputs.iter().any(|o| matches!(o, Output::Deadline(_)));
        assert!(has_transmit, "expected Transmit output");
        assert!(has_deadline, "expected Deadline output");
    }

    #[test]
    fn test_send_allocates_invoke_id() {
        let mut stack = default_stack();
        let _outputs = stack.process(Input::Send {
            service: read_property_service(),
            dest: dest(),
        });
        assert_eq!(stack.in_flight.len(), 1);
    }

    #[test]
    fn test_send_256_exhausts_pool() {
        let mut stack = default_stack();
        // Fill all 256 invoke IDs.
        for _ in 0..256 {
            stack.process(Input::Send {
                service: read_property_service(),
                dest: dest(),
            });
        }
        // 257th should return an Error event.
        let outputs = stack.process(Input::Send {
            service: read_property_service(),
            dest: dest(),
        });
        let has_error = outputs
            .iter()
            .any(|o| matches!(o, Output::Event(BacnetEvent::Error { .. })));
        assert!(has_error, "expected InvokeIdExhausted error");
    }

    // ── Tick / retry ────────────────────────────────────────────────────────

    #[test]
    fn test_tick_before_timeout_returns_deadline() {
        let mut stack = default_stack();
        stack.process(Input::Send {
            service: read_property_service(),
            dest: dest(),
        });
        // Tick just before the timeout — should not retransmit.
        let outputs = stack.process(Input::Tick {
            now: stack.config.apdu_timeout_secs * 0.5,
        });
        let has_transmit = outputs.iter().any(|o| matches!(o, Output::Transmit { .. }));
        assert!(!has_transmit, "should not retransmit before timeout");
        let has_deadline = outputs.iter().any(|o| matches!(o, Output::Deadline(_)));
        assert!(has_deadline);
    }

    #[test]
    fn test_tick_at_timeout_retransmits() {
        let config = StackConfig {
            apdu_timeout_secs: 3.0,
            apdu_retries: 2,
            ..Default::default()
        };
        let mut stack = Stack::new(config);
        stack.process(Input::Send {
            service: read_property_service(),
            dest: dest(),
        });
        // First retry at t=3.0.
        let outputs = stack.process(Input::Tick { now: 3.0 });
        let has_transmit = outputs.iter().any(|o| matches!(o, Output::Transmit { .. }));
        assert!(has_transmit, "expected retransmit on first timeout");
        assert_eq!(stack.in_flight.len(), 1); // still in flight
    }

    #[test]
    fn test_tick_exhausts_retries_emits_timeout_event() {
        let config = StackConfig {
            apdu_timeout_secs: 3.0,
            apdu_retries: 2,
            ..Default::default()
        };
        let mut stack = Stack::new(config);
        stack.process(Input::Send {
            service: read_property_service(),
            dest: dest(),
        });
        // Trigger 3 ticks to exhaust 2 retries (retry 0→1 at t=3, retry 1→2 at t=6,
        // timeout at t=9 where retry_count(2) >= apdu_retries(2)).
        stack.process(Input::Tick { now: 3.0 }); // retry 1
        stack.process(Input::Tick { now: 6.0 }); // retry 2
        let outputs = stack.process(Input::Tick { now: 9.0 }); // exhausted
        let has_timeout = outputs
            .iter()
            .any(|o| matches!(o, Output::Event(BacnetEvent::Timeout { .. })));
        assert!(
            has_timeout,
            "expected Timeout event after retries exhausted"
        );
        assert_eq!(
            stack.in_flight.len(),
            0,
            "slot should be freed after timeout"
        );
    }

    // ── Received / response matching ────────────────────────────────────────

    #[test]
    fn test_received_complex_ack_emits_response() {
        let mut stack = default_stack();
        let _outputs = stack.process(Input::Send {
            service: read_property_service(),
            dest: dest(),
        });
        // Extract the invoke_id from the in-flight map (should be 0).
        let invoke_id = stack.in_flight.keys().next().unwrap().1;

        // Build a ComplexACK: [0x30, invoke_id, service_choice=12, data...]
        let apdu = vec![0x30u8, invoke_id, 12, 0xDE, 0xAD];
        let frame = make_received_frame(&apdu);

        let outputs = stack.process(Input::Received {
            data: frame,
            src: dest(),
        });
        let has_response = outputs
            .iter()
            .any(|o| matches!(o, Output::Event(BacnetEvent::Response { .. })));
        assert!(has_response, "expected Response event on ComplexACK");
        assert_eq!(stack.in_flight.len(), 0, "slot should be freed on response");
    }

    #[test]
    fn test_received_simple_ack_emits_response() {
        let mut stack = default_stack();
        stack.process(Input::Send {
            service: read_property_service(),
            dest: dest(),
        });
        let invoke_id = stack.in_flight.keys().next().unwrap().1;

        let apdu = vec![0x20u8, invoke_id, 15]; // SimpleACK, service 15
        let frame = make_received_frame(&apdu);
        let outputs = stack.process(Input::Received {
            data: frame,
            src: dest(),
        });
        assert!(outputs
            .iter()
            .any(|o| matches!(o, Output::Event(BacnetEvent::Response { .. }))));
        assert_eq!(stack.in_flight.len(), 0);
    }

    #[test]
    fn test_received_error_apdu_emits_error_event() {
        let mut stack = default_stack();
        stack.process(Input::Send {
            service: read_property_service(),
            dest: dest(),
        });
        let invoke_id = stack.in_flight.keys().next().unwrap().1;

        // Error APDU: [0x50, invoke_id, svc=12, class Enum, code Enum]
        let apdu = vec![0x50u8, invoke_id, 12, 0x91, 0x02, 0x91, 0x1F];
        let frame = make_received_frame(&apdu);
        let outputs = stack.process(Input::Received {
            data: frame,
            src: dest(),
        });
        assert!(outputs
            .iter()
            .any(|o| matches!(o, Output::Event(BacnetEvent::Error { .. }))));
        assert_eq!(stack.in_flight.len(), 0);
    }

    #[test]
    fn test_received_abort_emits_abort_event() {
        let mut stack = default_stack();
        stack.process(Input::Send {
            service: read_property_service(),
            dest: dest(),
        });
        let invoke_id = stack.in_flight.keys().next().unwrap().1;

        let apdu = vec![0x71u8, invoke_id, 0x04]; // Abort from server, reason 4
        let frame = make_received_frame(&apdu);
        let outputs = stack.process(Input::Received {
            data: frame,
            src: dest(),
        });
        assert!(outputs
            .iter()
            .any(|o| matches!(o, Output::Event(BacnetEvent::Abort { .. }))));
        assert_eq!(stack.in_flight.len(), 0);
    }

    #[test]
    fn test_invoke_id_reused_after_response() {
        let mut stack = default_stack();
        // Send and immediately respond to free invoke_id=0.
        stack.process(Input::Send {
            service: read_property_service(),
            dest: dest(),
        });
        let invoke_id = stack.in_flight.keys().next().unwrap().1;
        let ack = make_received_frame(&[0x30, invoke_id, 12]);
        stack.process(Input::Received {
            data: ack,
            src: dest(),
        });

        // After freeing, the pool hint advances; fill until 0 is reused.
        // Allocate a second request — it must succeed (pool not empty).
        let outputs = stack.process(Input::Send {
            service: read_property_service(),
            dest: dest(),
        });
        let has_transmit = outputs.iter().any(|o| matches!(o, Output::Transmit { .. }));
        assert!(
            has_transmit,
            "should successfully send after invoke ID freed"
        );
        assert_eq!(stack.in_flight.len(), 1);
    }

    #[test]
    fn test_received_i_am_emits_unconfirmed_event() {
        let mut stack = default_stack();
        // Build I-Am unconfirmed APDU:
        // PDU type 1 (unconfirmed) << 4 = 0x10 | 0 = 0x10
        // service choice = 0 (I-Am)
        // device-id: ObjectIdentifier app tag, Device,1234
        let oid = ObjectIdentifier::new(ObjectType::Device, 1234).to_u32();
        let mut apdu = vec![0x10u8, 0x00]; // UnconfirmedRequest, I-Am
        apdu.push(0xC4);
        apdu.extend_from_slice(&oid.to_be_bytes()); // OID
        apdu.push(0x22);
        apdu.push(0x05);
        apdu.push(0xC4); // max-apdu = 1476
        apdu.push(0x91);
        apdu.push(0x00); // segmentation = Both
        apdu.push(0x21);
        apdu.push(0x0F); // vendor-id = 15
        let frame = make_received_frame(&apdu);
        let outputs = stack.process(Input::Received {
            data: frame,
            src: dest(),
        });
        assert!(outputs.iter().any(|o| matches!(
            o,
            Output::Event(BacnetEvent::UnconfirmedReceived {
                message: UnconfirmedMessage::IAm { .. },
                ..
            })
        )));
    }
}
