use crate::codec::types::DecodeError;

/// BVLC function codes for BACnet/IP
pub const BVLC_TYPE: u8 = 0x81;
pub const BVLC_ORIGINAL_UNICAST_NPDU: u8 = 0x0A;
pub const BVLC_ORIGINAL_BROADCAST_NPDU: u8 = 0x0B;

/// The type of BVLC frame
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BvlcFunction {
    OriginalUnicastNpdu,
    OriginalBroadcastNpdu,
}

/// Encode an NPDU payload into a BVLC frame.
///
/// The BVLC header is 4 bytes:
///   - byte 0: `0x81` (BACnet/IP type)
///   - byte 1: function code
///   - bytes 2–3: total length (big-endian), including the 4-byte header
pub fn encode(function: BvlcFunction, npdu: &[u8]) -> Vec<u8> {
    let total_len = 4 + npdu.len();
    let function_code = match function {
        BvlcFunction::OriginalUnicastNpdu => BVLC_ORIGINAL_UNICAST_NPDU,
        BvlcFunction::OriginalBroadcastNpdu => BVLC_ORIGINAL_BROADCAST_NPDU,
    };
    let mut buf = Vec::with_capacity(total_len);
    buf.push(BVLC_TYPE);
    buf.push(function_code);
    buf.push((total_len >> 8) as u8);
    buf.push(total_len as u8);
    buf.extend_from_slice(npdu);
    buf
}

/// Result of decoding a BVLC frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BvlcFrame<'a> {
    pub function: BvlcFunction,
    /// The inner NPDU bytes (a slice into the original buffer).
    pub npdu: &'a [u8],
}

/// Decode a raw UDP payload into a `BvlcFrame`.
///
/// Returns `DecodeError::UnsupportedBvlcFunction` for unknown function codes.
pub fn decode(data: &[u8]) -> Result<BvlcFrame<'_>, DecodeError> {
    if data.len() < 4 {
        return Err(DecodeError::IncompleteData);
    }
    if data[0] != BVLC_TYPE {
        return Err(DecodeError::InvalidData);
    }
    let function_code = data[1];
    let length = ((data[2] as usize) << 8) | data[3] as usize;
    if data.len() < length {
        return Err(DecodeError::IncompleteData);
    }
    let function = match function_code {
        BVLC_ORIGINAL_UNICAST_NPDU => BvlcFunction::OriginalUnicastNpdu,
        BVLC_ORIGINAL_BROADCAST_NPDU => BvlcFunction::OriginalBroadcastNpdu,
        _ => return Err(DecodeError::UnsupportedBvlcFunction),
    };
    Ok(BvlcFrame {
        function,
        npdu: &data[4..length],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_unicast() {
        let npdu = [0x01, 0x04, 0x02, 0x03];
        let frame = encode(BvlcFunction::OriginalUnicastNpdu, &npdu);
        assert_eq!(&frame[0..4], &[0x81, 0x0A, 0x00, 0x08]);
        assert_eq!(&frame[4..], &npdu);
    }

    #[test]
    fn test_encode_broadcast() {
        let npdu = [0x01, 0x20];
        let frame = encode(BvlcFunction::OriginalBroadcastNpdu, &npdu);
        assert_eq!(&frame[0..4], &[0x81, 0x0B, 0x00, 0x06]);
        assert_eq!(&frame[4..], &npdu);
    }

    #[test]
    fn test_encode_empty_npdu() {
        let frame = encode(BvlcFunction::OriginalUnicastNpdu, &[]);
        assert_eq!(frame, [0x81, 0x0A, 0x00, 0x04]);
    }

    #[test]
    fn test_decode_unicast() {
        let data = [0x81, 0x0A, 0x00, 0x06, 0xAA, 0xBB];
        let frame = decode(&data).unwrap();
        assert_eq!(frame.function, BvlcFunction::OriginalUnicastNpdu);
        assert_eq!(frame.npdu, &[0xAA, 0xBB]);
    }

    #[test]
    fn test_decode_broadcast() {
        let data = [0x81, 0x0B, 0x00, 0x05, 0xFF];
        let frame = decode(&data).unwrap();
        assert_eq!(frame.function, BvlcFunction::OriginalBroadcastNpdu);
        assert_eq!(frame.npdu, &[0xFF]);
    }

    #[test]
    fn test_decode_unknown_function_returns_error() {
        let data = [0x81, 0x01, 0x00, 0x04]; // 0x01 = Register-Foreign-Device, not supported
        let err = decode(&data).unwrap_err();
        assert_eq!(err, DecodeError::UnsupportedBvlcFunction);
    }

    #[test]
    fn test_decode_wrong_type_byte_returns_invalid() {
        let data = [0x80, 0x0A, 0x00, 0x04];
        let err = decode(&data).unwrap_err();
        assert_eq!(err, DecodeError::InvalidData);
    }

    #[test]
    fn test_decode_truncated_returns_incomplete() {
        let data = [0x81, 0x0A, 0x00];
        let err = decode(&data).unwrap_err();
        assert_eq!(err, DecodeError::IncompleteData);
    }

    #[test]
    fn test_roundtrip_unicast() {
        let npdu = vec![0x01, 0x04, 0xDE, 0xAD, 0xBE, 0xEF];
        let encoded = encode(BvlcFunction::OriginalUnicastNpdu, &npdu);
        let frame = decode(&encoded).unwrap();
        assert_eq!(frame.function, BvlcFunction::OriginalUnicastNpdu);
        assert_eq!(frame.npdu, npdu.as_slice());
    }

    #[test]
    fn test_roundtrip_broadcast() {
        let npdu = vec![0x01, 0x20, 0x00];
        let encoded = encode(BvlcFunction::OriginalBroadcastNpdu, &npdu);
        let frame = decode(&encoded).unwrap();
        assert_eq!(frame.function, BvlcFunction::OriginalBroadcastNpdu);
        assert_eq!(frame.npdu, npdu.as_slice());
    }
}
