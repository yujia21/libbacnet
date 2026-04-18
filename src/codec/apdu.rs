use crate::codec::types::DecodeError;

// PDU type values (upper nibble of first byte, shifted right 4)
pub const PDU_TYPE_CONFIRMED_REQUEST: u8 = 0x00;
pub const PDU_TYPE_UNCONFIRMED_REQUEST: u8 = 0x01;
pub const PDU_TYPE_SIMPLE_ACK: u8 = 0x02;
pub const PDU_TYPE_COMPLEX_ACK: u8 = 0x03;
pub const PDU_TYPE_SEGMENT_ACK: u8 = 0x04;
pub const PDU_TYPE_ERROR: u8 = 0x05;
pub const PDU_TYPE_REJECT: u8 = 0x06;
pub const PDU_TYPE_ABORT: u8 = 0x07;

// ConfirmedRequest first-byte flag bits (bits 3-0 of byte 0)
const CR_FLAG_SEG: u8 = 0x08; // segmented message
const CR_FLAG_MOR: u8 = 0x04; // more follows
const CR_FLAG_SA: u8 = 0x02; // segmented-response-accepted

// ComplexACK first-byte flag bits
const CA_FLAG_SEG: u8 = 0x08;
const CA_FLAG_MOR: u8 = 0x04;

// SegmentACK first-byte flag bits
const SA_FLAG_NAK: u8 = 0x02; // negative acknowledgement
const SA_FLAG_SRV: u8 = 0x01; // server flag

// Abort first-byte flag bit
const ABORT_FLAG_SRV: u8 = 0x01; // sent by server

// ─────────────────────────────────────────────────────────────────────────────
// Shared types
// ─────────────────────────────────────────────────────────────────────────────

/// Max-segments-accepted encoding (3 bits, byte 1 upper nibble of ConfirmedRequest).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MaxSegments {
    #[default]
    Unspecified = 0,
    Two = 1,
    Four = 2,
    Eight = 3,
    Sixteen = 4,
    ThirtyTwo = 5,
    SixtyFour = 6,
    MoreThan64 = 7,
}

impl MaxSegments {
    pub fn from_u8(v: u8) -> Self {
        match (v >> 4) & 0x07 {
            0 => MaxSegments::Unspecified,
            1 => MaxSegments::Two,
            2 => MaxSegments::Four,
            3 => MaxSegments::Eight,
            4 => MaxSegments::Sixteen,
            5 => MaxSegments::ThirtyTwo,
            6 => MaxSegments::SixtyFour,
            _ => MaxSegments::MoreThan64,
        }
    }
    pub fn to_u8(self) -> u8 {
        (self as u8) << 4
    }
}

/// Max-APDU-length-accepted encoding (4 bits, lower nibble of byte 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MaxApduLength {
    #[default]
    Up50 = 0,
    Up128 = 1,
    Up206 = 2,
    Up480 = 3,
    Up1024 = 4,
    Up1476 = 5,
}

impl MaxApduLength {
    pub fn from_u8(v: u8) -> Self {
        match v & 0x0F {
            0 => MaxApduLength::Up50,
            1 => MaxApduLength::Up128,
            2 => MaxApduLength::Up206,
            3 => MaxApduLength::Up480,
            4 => MaxApduLength::Up1024,
            _ => MaxApduLength::Up1476,
        }
    }
    pub fn to_u8(self) -> u8 {
        self as u8
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// ConfirmedRequest
// ─────────────────────────────────────────────────────────────────────────────

/// Parameters for encoding a ConfirmedRequest APDU.
#[derive(Debug, Clone)]
pub struct ConfirmedRequestParams<'a> {
    /// Whether the sender can accept a segmented response.
    pub segmented_response_accepted: bool,
    pub max_segments: MaxSegments,
    pub max_apdu_length: MaxApduLength,
    pub invoke_id: u8,
    /// Segmentation fields — None for unsegmented messages.
    pub segmentation: Option<SegmentationHeader>,
    /// Service choice byte (e.g. 12 = ReadProperty).
    pub service_choice: u8,
    /// Encoded service-request bytes.
    pub service_data: &'a [u8],
}

/// Segmentation header fields shared between ConfirmedRequest and ComplexACK.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SegmentationHeader {
    pub sequence_number: u8,
    pub proposed_window_size: u8,
    /// True when more segments follow.
    pub more_follows: bool,
}

/// Encode a ConfirmedRequest APDU.
///
/// Layout (unsegmented):
///   [0] PDU-type(0x00) | flags
///   [1] max-segs | max-apdu
///   [2] invoke-id
///   [3] service-choice
///   [4..] service-data
///
/// Layout (segmented):
///   [0] PDU-type | SEG | [MOR]
///   [1] max-segs | max-apdu
///   [2] invoke-id
///   [3] sequence-number
///   [4] proposed-window-size
///   [5] service-choice
///   [6..] service-data
pub fn encode_confirmed_request(params: &ConfirmedRequestParams<'_>) -> Vec<u8> {
    let mut byte0: u8 = PDU_TYPE_CONFIRMED_REQUEST << 4;
    if params.segmented_response_accepted {
        byte0 |= CR_FLAG_SA;
    }
    if let Some(seg) = &params.segmentation {
        byte0 |= CR_FLAG_SEG;
        if seg.more_follows {
            byte0 |= CR_FLAG_MOR;
        }
    }

    let byte1 = params.max_segments.to_u8() | params.max_apdu_length.to_u8();

    let mut buf = Vec::new();
    buf.push(byte0);
    buf.push(byte1);
    buf.push(params.invoke_id);
    if let Some(seg) = &params.segmentation {
        buf.push(seg.sequence_number);
        buf.push(seg.proposed_window_size);
    }
    buf.push(params.service_choice);
    buf.extend_from_slice(params.service_data);
    buf
}

// ─────────────────────────────────────────────────────────────────────────────
// ComplexACK
// ─────────────────────────────────────────────────────────────────────────────

/// Decoded ComplexACK APDU.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComplexAck<'a> {
    pub invoke_id: u8,
    pub service_ack_choice: u8,
    pub segmentation: Option<SegmentationHeader>,
    /// Raw service-ACK data bytes.
    pub service_data: &'a [u8],
}

/// Decode a ComplexACK APDU.
///
/// Layout (unsegmented):
///   [0] 0x30 | flags
///   [1] invoke-id
///   [2] service-ack-choice
///   [3..] service-data
///
/// Layout (segmented):
///   [0] 0x30 | SEG | [MOR]
///   [1] invoke-id
///   [2] sequence-number
///   [3] proposed-window-size
///   [4] service-ack-choice
///   [5..] service-data
pub fn decode_complex_ack(data: &[u8]) -> Result<ComplexAck<'_>, DecodeError> {
    if data.len() < 3 {
        return Err(DecodeError::IncompleteData);
    }
    let byte0 = data[0];
    if (byte0 >> 4) != PDU_TYPE_COMPLEX_ACK {
        return Err(DecodeError::InvalidData);
    }
    let is_segmented = (byte0 & CA_FLAG_SEG) != 0;
    let more_follows = (byte0 & CA_FLAG_MOR) != 0;

    let invoke_id = data[1];

    if is_segmented {
        if data.len() < 5 {
            return Err(DecodeError::IncompleteData);
        }
        let seg = SegmentationHeader {
            sequence_number: data[2],
            proposed_window_size: data[3],
            more_follows,
        };
        let service_ack_choice = data[4];
        Ok(ComplexAck {
            invoke_id,
            service_ack_choice,
            segmentation: Some(seg),
            service_data: &data[5..],
        })
    } else {
        let service_ack_choice = data[2];
        Ok(ComplexAck {
            invoke_id,
            service_ack_choice,
            segmentation: None,
            service_data: &data[3..],
        })
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// SimpleACK
// ─────────────────────────────────────────────────────────────────────────────

/// Decoded SimpleACK APDU.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimpleAck {
    pub invoke_id: u8,
    pub service_ack_choice: u8,
}

/// Decode a SimpleACK APDU.
///
/// Layout: [0] 0x20  [1] invoke-id  [2] service-ack-choice
pub fn decode_simple_ack(data: &[u8]) -> Result<SimpleAck, DecodeError> {
    if data.len() < 3 {
        return Err(DecodeError::IncompleteData);
    }
    if (data[0] >> 4) != PDU_TYPE_SIMPLE_ACK {
        return Err(DecodeError::InvalidData);
    }
    Ok(SimpleAck {
        invoke_id: data[1],
        service_ack_choice: data[2],
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// SegmentACK
// ─────────────────────────────────────────────────────────────────────────────

/// SegmentACK APDU (both directions).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentAck {
    /// True = negative acknowledgement.
    pub negative_ack: bool,
    /// True = sent by server.
    pub server: bool,
    pub invoke_id: u8,
    pub sequence_number: u8,
    pub actual_window_size: u8,
}

/// Encode a SegmentACK APDU.
///
/// Layout: [0] 0x40 | flags  [1] invoke-id  [2] seq  [3] window
pub fn encode_segment_ack(ack: &SegmentAck) -> Vec<u8> {
    let mut byte0: u8 = PDU_TYPE_SEGMENT_ACK << 4;
    if ack.negative_ack {
        byte0 |= SA_FLAG_NAK;
    }
    if ack.server {
        byte0 |= SA_FLAG_SRV;
    }
    vec![
        byte0,
        ack.invoke_id,
        ack.sequence_number,
        ack.actual_window_size,
    ]
}

/// Decode a SegmentACK APDU.
pub fn decode_segment_ack(data: &[u8]) -> Result<SegmentAck, DecodeError> {
    if data.len() < 4 {
        return Err(DecodeError::IncompleteData);
    }
    let byte0 = data[0];
    if (byte0 >> 4) != PDU_TYPE_SEGMENT_ACK {
        return Err(DecodeError::InvalidData);
    }
    Ok(SegmentAck {
        negative_ack: (byte0 & SA_FLAG_NAK) != 0,
        server: (byte0 & SA_FLAG_SRV) != 0,
        invoke_id: data[1],
        sequence_number: data[2],
        actual_window_size: data[3],
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Error APDU
// ─────────────────────────────────────────────────────────────────────────────

/// Decoded Error APDU.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorApdu {
    pub invoke_id: u8,
    pub service_choice: u8,
    /// Error class (BACnet Enumerated, decoded as raw u32).
    pub error_class: u32,
    /// Error code (BACnet Enumerated, decoded as raw u32).
    pub error_code: u32,
}

/// Decode an Error APDU.
///
/// Layout:
///   [0] 0x50   [1] invoke-id   [2] service-choice
///   [3..] error-class (Enumerated) + error-code (Enumerated)
pub fn decode_error_apdu(data: &[u8]) -> Result<ErrorApdu, DecodeError> {
    if data.len() < 3 {
        return Err(DecodeError::IncompleteData);
    }
    if (data[0] >> 4) != PDU_TYPE_ERROR {
        return Err(DecodeError::InvalidData);
    }
    let invoke_id = data[1];
    let service_choice = data[2];

    // Parse two consecutive BACnet Enumerated values from data[3..]
    let rest = &data[3..];
    let (error_class, consumed1) = decode_enumerated_value(rest)?;
    let (error_code, _) = decode_enumerated_value(&rest[consumed1..])?;

    Ok(ErrorApdu {
        invoke_id,
        service_choice,
        error_class,
        error_code,
    })
}

/// Decode a BACnet Enumerated application-tagged value.
/// Returns (value, bytes_consumed).
fn decode_enumerated_value(data: &[u8]) -> Result<(u32, usize), DecodeError> {
    if data.is_empty() {
        return Err(DecodeError::IncompleteData);
    }
    let tag = data[0];
    let tag_number = (tag >> 4) & 0x0F;
    if tag_number != 9 {
        return Err(DecodeError::InvalidData);
    }
    let len = (tag & 0x07) as usize;
    if data.len() < 1 + len {
        return Err(DecodeError::IncompleteData);
    }
    let mut value = 0u32;
    for i in 0..len {
        value = (value << 8) | data[1 + i] as u32;
    }
    Ok((value, 1 + len))
}

// ─────────────────────────────────────────────────────────────────────────────
// Abort APDU
// ─────────────────────────────────────────────────────────────────────────────

/// Decoded Abort APDU.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AbortApdu {
    /// True = sent by server.
    pub server: bool,
    pub invoke_id: u8,
    pub abort_reason: u8,
}

/// Decode an Abort APDU.
///
/// Layout: [0] 0x70 | server  [1] invoke-id  [2] abort-reason
pub fn decode_abort(data: &[u8]) -> Result<AbortApdu, DecodeError> {
    if data.len() < 3 {
        return Err(DecodeError::IncompleteData);
    }
    if (data[0] >> 4) != PDU_TYPE_ABORT {
        return Err(DecodeError::InvalidData);
    }
    Ok(AbortApdu {
        server: (data[0] & ABORT_FLAG_SRV) != 0,
        invoke_id: data[1],
        abort_reason: data[2],
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Reject APDU
// ─────────────────────────────────────────────────────────────────────────────

/// Decoded Reject APDU.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RejectApdu {
    pub invoke_id: u8,
    pub reject_reason: u8,
}

/// Decode a Reject APDU.
///
/// Layout: [0] 0x60  [1] invoke-id  [2] reject-reason
pub fn decode_reject(data: &[u8]) -> Result<RejectApdu, DecodeError> {
    if data.len() < 3 {
        return Err(DecodeError::IncompleteData);
    }
    if (data[0] >> 4) != PDU_TYPE_REJECT {
        return Err(DecodeError::InvalidData);
    }
    Ok(RejectApdu {
        invoke_id: data[1],
        reject_reason: data[2],
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── ConfirmedRequest ────────────────────────────────────────────────────

    #[test]
    fn test_confirmed_request_unsegmented() {
        let service_data = [0x0C, 0x00, 0x00, 0x00, 0x05, 0x19, 0x55];
        let params = ConfirmedRequestParams {
            segmented_response_accepted: true,
            max_segments: MaxSegments::Unspecified,
            max_apdu_length: MaxApduLength::Up1476,
            invoke_id: 1,
            segmentation: None,
            service_choice: 12, // ReadProperty
            service_data: &service_data,
        };
        let apdu = encode_confirmed_request(&params);
        // byte0: PDU_TYPE(0)<<4 | SA(0x02) = 0x02
        assert_eq!(apdu[0], 0x02);
        // byte1: max_segments(0<<4) | max_apdu(5) = 0x05
        assert_eq!(apdu[1], 0x05);
        // invoke id
        assert_eq!(apdu[2], 0x01);
        // service choice = 12
        assert_eq!(apdu[3], 12);
        // service data
        assert_eq!(&apdu[4..], &service_data);
    }

    #[test]
    fn test_confirmed_request_segmented() {
        let params = ConfirmedRequestParams {
            segmented_response_accepted: false,
            max_segments: MaxSegments::Unspecified,
            max_apdu_length: MaxApduLength::Up1476,
            invoke_id: 3,
            segmentation: Some(SegmentationHeader {
                sequence_number: 0,
                proposed_window_size: 4,
                more_follows: true,
            }),
            service_choice: 14, // ReadPropertyMultiple
            service_data: &[0xAA],
        };
        let apdu = encode_confirmed_request(&params);
        // byte0: SEG(0x08) | MOR(0x04) = 0x0C
        assert_eq!(apdu[0], 0x0C);
        assert_eq!(apdu[2], 3); // invoke id
        assert_eq!(apdu[3], 0); // sequence number
        assert_eq!(apdu[4], 4); // window size
        assert_eq!(apdu[5], 14); // service choice
        assert_eq!(apdu[6], 0xAA);
    }

    #[test]
    fn test_confirmed_request_wrong_pdu_type_not_decoded() {
        // encode then check the PDU type nibble is 0
        let apdu = encode_confirmed_request(&ConfirmedRequestParams {
            segmented_response_accepted: false,
            max_segments: MaxSegments::Unspecified,
            max_apdu_length: MaxApduLength::Up50,
            invoke_id: 0,
            segmentation: None,
            service_choice: 12,
            service_data: &[],
        });
        assert_eq!(apdu[0] >> 4, PDU_TYPE_CONFIRMED_REQUEST);
    }

    // ── ComplexACK ──────────────────────────────────────────────────────────

    #[test]
    fn test_complex_ack_unsegmented() {
        // [0] 0x30  [1] invoke=5  [2] service=12  [3..] data
        let data = [0x30, 0x05, 0x0C, 0xDE, 0xAD];
        let ack = decode_complex_ack(&data).unwrap();
        assert_eq!(ack.invoke_id, 5);
        assert_eq!(ack.service_ack_choice, 12);
        assert!(ack.segmentation.is_none());
        assert_eq!(ack.service_data, &[0xDE, 0xAD]);
    }

    #[test]
    fn test_complex_ack_segmented() {
        // [0] 0x38 (SEG set)  [1] invoke=2  [2] seq=0  [3] win=4  [4] svc=12  [5..] data
        let data = [0x38, 0x02, 0x00, 0x04, 0x0C, 0xFF];
        let ack = decode_complex_ack(&data).unwrap();
        assert_eq!(ack.invoke_id, 2);
        assert_eq!(ack.service_ack_choice, 12);
        let seg = ack.segmentation.unwrap();
        assert_eq!(seg.sequence_number, 0);
        assert_eq!(seg.proposed_window_size, 4);
        assert!(!seg.more_follows);
        assert_eq!(ack.service_data, &[0xFF]);
    }

    #[test]
    fn test_complex_ack_wrong_type() {
        let data = [0x20, 0x01, 0x0C];
        assert_eq!(
            decode_complex_ack(&data).unwrap_err(),
            DecodeError::InvalidData
        );
    }

    #[test]
    fn test_complex_ack_truncated() {
        let data = [0x30, 0x01];
        assert_eq!(
            decode_complex_ack(&data).unwrap_err(),
            DecodeError::IncompleteData
        );
    }

    // ── SimpleACK ───────────────────────────────────────────────────────────

    #[test]
    fn test_simple_ack() {
        // [0] 0x20  [1] invoke=7  [2] service=15
        let data = [0x20, 0x07, 0x0F];
        let ack = decode_simple_ack(&data).unwrap();
        assert_eq!(ack.invoke_id, 7);
        assert_eq!(ack.service_ack_choice, 15);
    }

    #[test]
    fn test_simple_ack_wrong_type() {
        let data = [0x30, 0x01, 0x0C];
        assert_eq!(
            decode_simple_ack(&data).unwrap_err(),
            DecodeError::InvalidData
        );
    }

    #[test]
    fn test_simple_ack_truncated() {
        assert_eq!(
            decode_simple_ack(&[0x20]).unwrap_err(),
            DecodeError::IncompleteData
        );
    }

    // ── SegmentACK ──────────────────────────────────────────────────────────

    #[test]
    fn test_segment_ack_roundtrip() {
        let ack = SegmentAck {
            negative_ack: false,
            server: true,
            invoke_id: 4,
            sequence_number: 2,
            actual_window_size: 8,
        };
        let encoded = encode_segment_ack(&ack);
        // byte0: 0x40 | SRV(0x01) = 0x41
        assert_eq!(encoded[0], 0x41);
        let decoded = decode_segment_ack(&encoded).unwrap();
        assert_eq!(decoded, ack);
    }

    #[test]
    fn test_segment_ack_negative() {
        let ack = SegmentAck {
            negative_ack: true,
            server: false,
            invoke_id: 1,
            sequence_number: 0,
            actual_window_size: 4,
        };
        let encoded = encode_segment_ack(&ack);
        // byte0: 0x40 | NAK(0x02) = 0x42
        assert_eq!(encoded[0], 0x42);
        let decoded = decode_segment_ack(&encoded).unwrap();
        assert_eq!(decoded, ack);
    }

    #[test]
    fn test_segment_ack_wrong_type() {
        let data = [0x20, 0x01, 0x00, 0x04];
        assert_eq!(
            decode_segment_ack(&data).unwrap_err(),
            DecodeError::InvalidData
        );
    }

    // ── Error APDU ──────────────────────────────────────────────────────────

    #[test]
    fn test_error_apdu() {
        // [0] 0x50  [1] invoke=1  [2] svc=12
        // error-class: Enumerated tag=9, len=1, value=2 (Object)
        // error-code:  Enumerated tag=9, len=1, value=31 (unknown-object)
        let data = [0x50, 0x01, 0x0C, 0x91, 0x02, 0x91, 0x1F];
        let err = decode_error_apdu(&data).unwrap();
        assert_eq!(err.invoke_id, 1);
        assert_eq!(err.service_choice, 12);
        assert_eq!(err.error_class, 2);
        assert_eq!(err.error_code, 31);
    }

    #[test]
    fn test_error_apdu_wrong_type() {
        let data = [0x20, 0x01, 0x0C, 0x91, 0x02, 0x91, 0x1F];
        assert_eq!(
            decode_error_apdu(&data).unwrap_err(),
            DecodeError::InvalidData
        );
    }

    #[test]
    fn test_error_apdu_truncated() {
        assert_eq!(
            decode_error_apdu(&[0x50, 0x01]).unwrap_err(),
            DecodeError::IncompleteData
        );
    }

    // ── Abort APDU ──────────────────────────────────────────────────────────

    #[test]
    fn test_abort_apdu_server() {
        // [0] 0x71 (server flag)  [1] invoke=3  [2] reason=1
        let data = [0x71, 0x03, 0x01];
        let abort = decode_abort(&data).unwrap();
        assert!(abort.server);
        assert_eq!(abort.invoke_id, 3);
        assert_eq!(abort.abort_reason, 1);
    }

    #[test]
    fn test_abort_apdu_client() {
        let data = [0x70, 0x05, 0x04];
        let abort = decode_abort(&data).unwrap();
        assert!(!abort.server);
        assert_eq!(abort.invoke_id, 5);
        assert_eq!(abort.abort_reason, 4);
    }

    #[test]
    fn test_abort_apdu_wrong_type() {
        let data = [0x50, 0x01, 0x00];
        assert_eq!(decode_abort(&data).unwrap_err(), DecodeError::InvalidData);
    }

    // ── Reject APDU ─────────────────────────────────────────────────────────

    #[test]
    fn test_reject_apdu() {
        // [0] 0x60  [1] invoke=2  [2] reason=3
        let data = [0x60, 0x02, 0x03];
        let rej = decode_reject(&data).unwrap();
        assert_eq!(rej.invoke_id, 2);
        assert_eq!(rej.reject_reason, 3);
    }

    #[test]
    fn test_reject_apdu_wrong_type() {
        let data = [0x70, 0x01, 0x00];
        assert_eq!(decode_reject(&data).unwrap_err(), DecodeError::InvalidData);
    }

    #[test]
    fn test_reject_apdu_truncated() {
        assert_eq!(
            decode_reject(&[0x60]).unwrap_err(),
            DecodeError::IncompleteData
        );
    }
}
