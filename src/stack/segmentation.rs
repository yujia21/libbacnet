//! Segmentation state for send and receive sides.
//!
//! BACnet segmentation reference: ASHRAE 135-2020 §5.4

use crate::codec::apdu::{
    encode_confirmed_request, encode_segment_ack, ConfirmedRequestParams, MaxApduLength,
    MaxSegments, SegmentAck, SegmentationHeader,
};
use crate::codec::bvlc::{self, BvlcFunction};
use crate::codec::npdu;
use crate::stack::addr::BacnetAddr;

// ─────────────────────────────────────────────────────────────────────────────
// Constants
// ─────────────────────────────────────────────────────────────────────────────

/// Default proposed window size (number of segments per window).
pub const DEFAULT_WINDOW_SIZE: u8 = 4;

/// Segmentation timeout in seconds (separate from APDU timeout).
pub const SEG_TIMEOUT_SECS: f64 = 2.0;

/// Magic abort reason byte for reassembly timeout (BACnet abort reason 5).
pub const ABORT_REASON_REASSEMBLY_TIMEOUT: u8 = 5;

// ─────────────────────────────────────────────────────────────────────────────
// Send-side segmentation state
// ─────────────────────────────────────────────────────────────────────────────

/// All state needed to send a large segmented ConfirmedRequest.
#[derive(Debug, Clone)]
pub struct SendSegState {
    /// The full service-data payload (already encoded, without APDU header).
    pub service_data: Vec<u8>,
    /// Service choice byte.
    pub service_choice: u8,
    /// Invoke ID for this request.
    pub invoke_id: u8,
    /// Destination.
    pub dest: BacnetAddr,
    /// Fragment size in bytes (max APDU payload per segment).
    pub fragment_size: usize,
    /// Total number of fragments.
    pub total_fragments: usize,
    /// Index of the first unacknowledged fragment (window start).
    pub window_start: usize,
    /// Negotiated window size (from SegACK or default).
    pub window_size: u8,
    /// Timestamp of when the current window was sent.
    pub window_sent_at: f64,
}

impl SendSegState {
    /// Create a new send-segmentation state and return the first window of
    /// encoded BVLC frames to transmit.
    pub fn new(
        service_data: Vec<u8>,
        service_choice: u8,
        invoke_id: u8,
        dest: BacnetAddr,
        fragment_size: usize,
        now: f64,
    ) -> (Self, Vec<Vec<u8>>) {
        let total_fragments = service_data.len().div_ceil(fragment_size);
        let window_size = DEFAULT_WINDOW_SIZE;

        let state = Self {
            service_data,
            service_choice,
            invoke_id,
            dest,
            fragment_size,
            total_fragments,
            window_start: 0,
            window_size,
            window_sent_at: now,
        };

        let frames = state.build_window(0, window_size);
        (state, frames)
    }

    /// Build a window of encoded frames starting at `start_seq`.
    pub fn build_window(&self, start_seq: usize, window_size: u8) -> Vec<Vec<u8>> {
        let end_seq = (start_seq + window_size as usize).min(self.total_fragments);
        (start_seq..end_seq)
            .map(|seq| self.build_frame(seq))
            .collect()
    }

    /// Build a single fragment frame for sequence number `seq`.
    fn build_frame(&self, seq: usize) -> Vec<u8> {
        let start = seq * self.fragment_size;
        let end = (start + self.fragment_size).min(self.service_data.len());
        let fragment = &self.service_data[start..end];
        let more_follows = seq + 1 < self.total_fragments;

        let apdu = encode_confirmed_request(&ConfirmedRequestParams {
            segmented_response_accepted: true,
            max_segments: MaxSegments::Unspecified,
            max_apdu_length: MaxApduLength::Up1476,
            invoke_id: self.invoke_id,
            segmentation: Some(SegmentationHeader {
                sequence_number: seq as u8,
                proposed_window_size: self.window_size,
                more_follows,
            }),
            service_choice: self.service_choice,
            service_data: fragment,
        });

        let npdu_bytes = npdu::encode(&npdu::NpduEncodeParams {
            apdu: &apdu,
            data_expecting_reply: true,
            ..Default::default()
        });
        bvlc::encode(BvlcFunction::OriginalUnicastNpdu, &npdu_bytes)
    }

    /// Handle a received SegACK. Returns the next window of frames to transmit,
    /// or `None` if transmission is complete.
    ///
    /// `actual_window_size` is the window size granted by the receiver.
    pub fn handle_seg_ack(
        &mut self,
        sequence_number: u8,
        actual_window_size: u8,
        now: f64,
    ) -> Option<Vec<Vec<u8>>> {
        let acked_seq = sequence_number as usize;
        // SegACK acknowledges all segments up to and including `sequence_number`.
        let new_window_start = acked_seq + 1;
        if new_window_start >= self.total_fragments {
            // All segments acknowledged.
            return None;
        }
        self.window_start = new_window_start;
        self.window_size = actual_window_size;
        self.window_sent_at = now;
        Some(self.build_window(new_window_start, actual_window_size))
    }

    /// Check whether the current window has timed out.
    pub fn is_window_timed_out(&self, now: f64) -> bool {
        now >= self.window_sent_at + SEG_TIMEOUT_SECS
    }

    /// Retransmit the current window (for 6.4).
    pub fn retransmit_window(&mut self, now: f64) -> Vec<Vec<u8>> {
        self.window_sent_at = now;
        self.build_window(self.window_start, self.window_size)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Receive-side segmentation state
// ─────────────────────────────────────────────────────────────────────────────

/// Fragment buffer for one incoming segmented response.
#[derive(Debug, Clone)]
pub struct RecvSegState {
    /// Accumulated fragment payloads keyed by sequence number.
    pub fragments: Vec<Option<Vec<u8>>>,
    /// Total number of fragments expected (set when last fragment arrives).
    pub total_fragments: Option<usize>,
    /// Window size to acknowledge (from the sender's proposed window).
    pub window_size: u8,
    /// Sequence number of the first fragment in the current window.
    pub window_start: usize,
    /// Timestamp when this state was created or last fragment was received.
    pub last_activity: f64,
    /// Max buffer bytes allowed (from StackConfig).
    pub max_buffer: usize,
    /// Bytes accumulated so far.
    pub bytes_accumulated: usize,
}

impl RecvSegState {
    pub fn new(window_size: u8, max_buffer: usize, now: f64) -> Self {
        Self {
            fragments: Vec::new(),
            total_fragments: None,
            window_size,
            window_start: 0,
            last_activity: now,
            max_buffer,
            bytes_accumulated: 0,
        }
    }

    /// Accept a fragment. Returns the action the stack should take.
    #[allow(clippy::too_many_arguments)]
    pub fn accept_fragment(
        &mut self,
        sequence_number: u8,
        more_follows: bool,
        proposed_window_size: u8,
        data: Vec<u8>,
        _src: BacnetAddr,
        invoke_id: u8,
        now: f64,
    ) -> RecvAction {
        let seq = sequence_number as usize;

        // Grow fragment buffer as needed.
        if seq >= self.fragments.len() {
            self.fragments.resize(seq + 1, None);
        }

        if self.fragments[seq].is_none() {
            self.bytes_accumulated += data.len();
            if self.bytes_accumulated > self.max_buffer {
                return RecvAction::Abort(ABORT_REASON_REASSEMBLY_TIMEOUT);
            }
            self.fragments[seq] = Some(data);
        }

        self.last_activity = now;
        self.window_size = proposed_window_size;

        if !more_follows {
            self.total_fragments = Some(seq + 1);
        }

        // Determine if this is the last fragment of the current window.
        let window_end = self.window_start + self.window_size as usize - 1;
        let is_window_complete = seq >= window_end || !more_follows;

        if is_window_complete {
            // Emit a SegACK for this window.
            let seg_ack = encode_segment_ack(&SegmentAck {
                negative_ack: false,
                server: false,
                invoke_id,
                sequence_number,
                actual_window_size: self.window_size,
            });
            let npdu_bytes = npdu::encode(&npdu::NpduEncodeParams {
                apdu: &seg_ack,
                data_expecting_reply: false,
                ..Default::default()
            });
            let frame = bvlc::encode(BvlcFunction::OriginalUnicastNpdu, &npdu_bytes);
            self.window_start = seq + 1;

            // Check if fully reassembled.
            if let Some(total) = self.total_fragments {
                if self.fragments.len() == total && self.fragments.iter().all(|f| f.is_some()) {
                    let payload: Vec<u8> = self
                        .fragments
                        .iter()
                        .filter_map(|f| f.as_ref())
                        .flat_map(|f| f.iter().copied())
                        .collect();
                    return RecvAction::Complete {
                        seg_ack_frame: frame,
                        payload,
                    };
                }
            }

            return RecvAction::SendSegAck(frame);
        }

        RecvAction::Continue
    }

    /// Returns true if this reassembly has timed out.
    pub fn is_timed_out(&self, now: f64) -> bool {
        now >= self.last_activity + SEG_TIMEOUT_SECS
    }
}

/// What the stack should do after accepting a fragment.
#[derive(Debug)]
pub enum RecvAction {
    /// Nothing to emit yet — waiting for more fragments in this window.
    Continue,
    /// Emit this SegACK frame — window complete but more fragments expected.
    SendSegAck(Vec<u8>),
    /// All fragments received. Emit SegACK then fire Response event.
    Complete {
        seg_ack_frame: Vec<u8>,
        payload: Vec<u8>,
    },
    /// Buffer overflow or sequence error — abort.
    Abort(u8),
}

// ─────────────────────────────────────────────────────────────────────────────
// Helper: split a raw service-data buffer into fragment slices
// ─────────────────────────────────────────────────────────────────────────────

/// Split `data` into chunks of at most `fragment_size` bytes.
pub fn split_into_fragments(data: &[u8], fragment_size: usize) -> Vec<&[u8]> {
    data.chunks(fragment_size).collect()
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::apdu::decode_segment_ack;
    use crate::codec::bvlc::decode as bvlc_decode;
    use crate::codec::npdu::decode as npdu_decode;

    fn dest() -> BacnetAddr {
        BacnetAddr::new([10, 0, 0, 1], 47808)
    }

    fn src() -> BacnetAddr {
        BacnetAddr::new([10, 0, 0, 2], 47808)
    }

    /// Decode a BVLC+NPDU frame and return the inner APDU bytes.
    fn extract_apdu(frame: &[u8]) -> Vec<u8> {
        let bvlc = bvlc_decode(frame).unwrap();
        let npdu = npdu_decode(bvlc.npdu).unwrap();
        npdu.apdu.to_vec()
    }

    // ── split_into_fragments ─────────────────────────────────────────────────

    #[test]
    fn test_split_even() {
        let data: Vec<u8> = (0..8).collect();
        let frags = split_into_fragments(&data, 4);
        assert_eq!(frags.len(), 2);
        assert_eq!(frags[0], &[0, 1, 2, 3]);
        assert_eq!(frags[1], &[4, 5, 6, 7]);
    }

    #[test]
    fn test_split_remainder() {
        let data: Vec<u8> = (0..10).collect();
        let frags = split_into_fragments(&data, 4);
        assert_eq!(frags.len(), 3);
        assert_eq!(frags[2], &[8, 9]);
    }

    // ── SendSegState ─────────────────────────────────────────────────────────

    #[test]
    fn test_send_first_window_fragment_count() {
        // 12 bytes, 4 bytes per fragment → 3 total fragments.
        // Window size = 4 → all 3 sent in first window.
        let data: Vec<u8> = (0u8..12).collect();
        let (state, frames) = SendSegState::new(data, 12, 0, dest(), 4, 0.0);
        assert_eq!(state.total_fragments, 3);
        assert_eq!(frames.len(), 3);
    }

    #[test]
    fn test_send_first_window_limited_by_window_size() {
        // 20 bytes, 4 bytes/fragment → 5 fragments. Window=4 → 4 frames.
        let data: Vec<u8> = (0u8..20).collect();
        let (state, frames) = SendSegState::new(data, 12, 0, dest(), 4, 0.0);
        assert_eq!(state.total_fragments, 5);
        assert_eq!(frames.len(), 4); // DEFAULT_WINDOW_SIZE = 4
    }

    #[test]
    fn test_send_more_follows_flag() {
        let data: Vec<u8> = (0u8..12).collect();
        let (_, frames) = SendSegState::new(data, 12, 0, dest(), 4, 0.0);
        // First two fragments have more_follows=true; last has more_follows=false.
        let apdu0 = extract_apdu(&frames[0]);
        let apdu2 = extract_apdu(&frames[2]);
        // SEG flag (0x08) set on all; MOR flag (0x04) set on first two only.
        assert_ne!(apdu0[0] & 0x04, 0, "first fragment should have MOR set");
        assert_eq!(apdu2[0] & 0x04, 0, "last fragment should not have MOR set");
    }

    #[test]
    fn test_send_sequence_numbers() {
        let data: Vec<u8> = (0u8..12).collect();
        let (_, frames) = SendSegState::new(data, 12, 0, dest(), 4, 0.0);
        for (expected_seq, frame) in frames.iter().enumerate() {
            let apdu = extract_apdu(frame);
            // Segmented ConfirmedRequest: byte[3] = sequence number
            assert_eq!(apdu[3], expected_seq as u8);
        }
    }

    #[test]
    fn test_handle_seg_ack_advances_window() {
        let data: Vec<u8> = (0u8..20).collect();
        let (mut state, _) = SendSegState::new(data, 12, 0, dest(), 4, 0.0);
        // ACK seq=3 (last of first window), window=4
        let frames = state.handle_seg_ack(3, 4, 1.0).unwrap();
        assert_eq!(state.window_start, 4);
        assert_eq!(frames.len(), 1); // only 1 fragment left (seq 4)
    }

    #[test]
    fn test_handle_seg_ack_complete_returns_none() {
        let data: Vec<u8> = (0u8..8).collect();
        let (mut state, _) = SendSegState::new(data, 12, 0, dest(), 4, 0.0);
        // Only 2 fragments total; ACK seq=1 → complete
        let result = state.handle_seg_ack(1, 4, 1.0);
        assert!(
            result.is_none(),
            "should return None when all segments acked"
        );
    }

    #[test]
    fn test_send_window_timeout() {
        let data: Vec<u8> = (0u8..20).collect();
        let (state, _) = SendSegState::new(data, 12, 0, dest(), 4, 0.0);
        assert!(!state.is_window_timed_out(1.0));
        assert!(state.is_window_timed_out(SEG_TIMEOUT_SECS + 0.01));
    }

    #[test]
    fn test_retransmit_window() {
        let data: Vec<u8> = (0u8..20).collect();
        let (mut state, original) = SendSegState::new(data, 12, 0, dest(), 4, 0.0);
        let retransmitted = state.retransmit_window(5.0);
        assert_eq!(retransmitted.len(), original.len());
        assert_eq!(state.window_sent_at, 5.0);
    }

    // ── RecvSegState ─────────────────────────────────────────────────────────

    /// Feed a sequence of 3 fragments into RecvSegState and check the result.
    #[test]
    fn test_recv_three_fragments_reassembly() {
        let mut state = RecvSegState::new(4, 1024 * 1024, 0.0);
        let invoke_id = 0u8;

        // Fragment 0 — not last of window, not last fragment
        let r0 = state.accept_fragment(0, true, 4, vec![0, 1, 2, 3], src(), invoke_id, 0.0);
        assert!(matches!(r0, RecvAction::Continue));

        // Fragment 1 — not last of window, not last fragment
        let r1 = state.accept_fragment(1, true, 4, vec![4, 5, 6, 7], src(), invoke_id, 0.0);
        assert!(matches!(r1, RecvAction::Continue));

        // Fragment 2 — last fragment (more_follows=false), which also ends the window
        let r2 = state.accept_fragment(2, false, 4, vec![8, 9], src(), invoke_id, 0.0);
        match r2 {
            RecvAction::Complete { payload, .. } => {
                assert_eq!(payload, vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
            }
            other => panic!("expected Complete, got {:?}", other),
        }
    }

    #[test]
    fn test_recv_seg_ack_at_window_boundary() {
        // Window size = 2; send 4 fragments across 2 windows.
        let mut state = RecvSegState::new(2, 1024 * 1024, 0.0);
        let invoke_id = 1u8;

        // Window 1: fragments 0 and 1
        let r0 = state.accept_fragment(0, true, 2, vec![0xAA], src(), invoke_id, 0.0);
        assert!(matches!(r0, RecvAction::Continue));
        let r1 = state.accept_fragment(1, true, 2, vec![0xBB], src(), invoke_id, 0.1);
        // seq=1 is end of window (window_start=0, window_size=2, window_end=1)
        let seg_ack_frame = match r1 {
            RecvAction::SendSegAck(f) => f,
            other => panic!("expected SendSegAck, got {:?}", other),
        };
        // Verify the SegACK frame decodes correctly
        let apdu = extract_apdu(&seg_ack_frame);
        let seg_ack = decode_segment_ack(&apdu).unwrap();
        assert_eq!(seg_ack.sequence_number, 1);
        assert_eq!(seg_ack.invoke_id, invoke_id);

        // Window 2: fragments 2 and 3 (last)
        let r2 = state.accept_fragment(2, true, 2, vec![0xCC], src(), invoke_id, 0.2);
        assert!(matches!(r2, RecvAction::Continue));
        let r3 = state.accept_fragment(3, false, 2, vec![0xDD], src(), invoke_id, 0.3);
        match r3 {
            RecvAction::Complete { payload, .. } => {
                assert_eq!(payload, vec![0xAA, 0xBB, 0xCC, 0xDD]);
            }
            other => panic!("expected Complete, got {:?}", other),
        }
    }

    #[test]
    fn test_recv_timeout() {
        let state = RecvSegState::new(4, 1024 * 1024, 0.0);
        assert!(!state.is_timed_out(1.0));
        assert!(state.is_timed_out(SEG_TIMEOUT_SECS + 0.01));
    }

    #[test]
    fn test_recv_buffer_overflow_aborts() {
        let mut state = RecvSegState::new(4, 10, 0.0); // 10-byte limit
                                                       // First fragment: 6 bytes — fine
        let r0 = state.accept_fragment(0, true, 4, vec![0u8; 6], src(), 0, 0.0);
        assert!(matches!(r0, RecvAction::Continue));
        // Second fragment: 6 bytes → total 12 > 10 → Abort
        let r1 = state.accept_fragment(1, true, 4, vec![0u8; 6], src(), 0, 0.0);
        assert!(matches!(r1, RecvAction::Abort(_)));
    }
}
