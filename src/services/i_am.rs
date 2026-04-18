//! I-Am and IAmRouterToNetwork decoders.
//!
//! I-Am unconfirmed service choice: 0
//! IAmRouterToNetwork is a network-layer message (NPDU message type 0x01).

use crate::codec::types::{DecodeError, ObjectIdentifier, PropertyValue};

pub const I_AM_SERVICE_CHOICE: u8 = 0;

/// Network-layer message type for IAmRouterToNetwork.
pub const MSG_I_AM_ROUTER_TO_NETWORK: u8 = 0x01;

// ─────────────────────────────────────────────────────────────────────────────
// I-Am decoder
// ─────────────────────────────────────────────────────────────────────────────

/// Segmentation support encoding (I-Am field).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Segmentation {
    Both = 0,
    Transmit = 1,
    Receive = 2,
    None = 3,
}

impl Segmentation {
    pub fn from_u32(v: u32) -> Self {
        match v {
            0 => Segmentation::Both,
            1 => Segmentation::Transmit,
            2 => Segmentation::Receive,
            _ => Segmentation::None,
        }
    }
}

/// Decoded I-Am message.
#[derive(Debug, Clone, PartialEq)]
pub struct IAmMessage {
    pub device_id: ObjectIdentifier,
    pub max_apdu_length_accepted: u32,
    pub segmentation_supported: Segmentation,
    pub vendor_id: u32,
}

/// Decode an I-Am unconfirmed request APDU service data.
///
/// Layout (all application-tagged):
///   ObjectIdentifier (app tag 12, 4 bytes)
///   Unsigned (max-apdu-length-accepted)
///   Enumerated (segmentation-supported)
///   Unsigned (vendor-id)
pub fn decode_i_am(data: &[u8]) -> Result<IAmMessage, DecodeError> {
    let mut offset = 0;

    // device-identifier: ObjectIdentifier app tag
    let (device_pv, n) = PropertyValue::decode(&data[offset..])?;
    offset += n;
    let device_id = match device_pv {
        PropertyValue::ObjectIdentifier(oid) => oid,
        _ => return Err(DecodeError::InvalidData),
    };

    // max-apdu-length-accepted: Unsigned
    let (max_apdu_pv, n) = PropertyValue::decode(&data[offset..])?;
    offset += n;
    let max_apdu_length_accepted = match max_apdu_pv {
        PropertyValue::Unsigned(v) => v,
        _ => return Err(DecodeError::InvalidData),
    };

    // segmentation-supported: Enumerated
    let (seg_pv, n) = PropertyValue::decode(&data[offset..])?;
    offset += n;
    let segmentation_supported = match seg_pv {
        PropertyValue::Enumerated(v) => Segmentation::from_u32(v),
        _ => return Err(DecodeError::InvalidData),
    };

    // vendor-id: Unsigned
    let (vendor_pv, _) = PropertyValue::decode(&data[offset..])?;
    let vendor_id = match vendor_pv {
        PropertyValue::Unsigned(v) => v,
        _ => return Err(DecodeError::InvalidData),
    };

    Ok(IAmMessage {
        device_id,
        max_apdu_length_accepted,
        segmentation_supported,
        vendor_id,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// IAmRouterToNetwork decoder
// ─────────────────────────────────────────────────────────────────────────────

/// Decoded IAmRouterToNetwork message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IAmRouterToNetworkMessage {
    /// List of network numbers reachable via this router.
    pub networks: Vec<u16>,
}

/// Decode an IAmRouterToNetwork network-layer message payload.
///
/// Layout: message-type byte (0x01) followed by pairs of 2-byte network numbers.
pub fn decode_i_am_router_to_network(
    data: &[u8],
) -> Result<IAmRouterToNetworkMessage, DecodeError> {
    if data.is_empty() {
        return Err(DecodeError::IncompleteData);
    }
    if data[0] != MSG_I_AM_ROUTER_TO_NETWORK {
        return Err(DecodeError::InvalidData);
    }
    let payload = &data[1..];
    if !payload.len().is_multiple_of(2) {
        return Err(DecodeError::InvalidData);
    }
    let networks = payload
        .chunks_exact(2)
        .map(|c| ((c[0] as u16) << 8) | c[1] as u16)
        .collect();
    Ok(IAmRouterToNetworkMessage { networks })
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::types::ObjectType;

    /// Build a known-good I-Am PDU service-data byte sequence.
    fn make_i_am(device_instance: u32, max_apdu: u32, seg: u32, vendor: u32) -> Vec<u8> {
        let mut buf = Vec::new();
        // device-id: ObjectIdentifier app tag 12 (0xC4), 4 bytes
        let oid = ObjectIdentifier::new(ObjectType::Device, device_instance).to_u32();
        buf.push(0xC4);
        buf.extend_from_slice(&oid.to_be_bytes());
        // max-apdu: Unsigned
        buf.push(0x22); // tag2, len2
        buf.push((max_apdu >> 8) as u8);
        buf.push(max_apdu as u8);
        // segmentation: Enumerated
        buf.push(0x91); // tag9, len1
        buf.push(seg as u8);
        // vendor-id: Unsigned
        buf.push(0x21); // tag2, len1
        buf.push(vendor as u8);
        buf
    }

    #[test]
    fn test_decode_i_am() {
        let data = make_i_am(1234, 1476, 0, 15);
        let msg = decode_i_am(&data).unwrap();
        assert_eq!(
            msg.device_id,
            ObjectIdentifier::new(ObjectType::Device, 1234)
        );
        assert_eq!(msg.max_apdu_length_accepted, 1476);
        assert_eq!(msg.segmentation_supported, Segmentation::Both);
        assert_eq!(msg.vendor_id, 15);
    }

    #[test]
    fn test_decode_i_am_segmentation_none() {
        let data = make_i_am(5, 480, 3, 1);
        let msg = decode_i_am(&data).unwrap();
        assert_eq!(msg.segmentation_supported, Segmentation::None);
    }

    #[test]
    fn test_decode_i_am_truncated() {
        let data = [0xC4, 0x02, 0x00, 0x00]; // only 4 bytes, incomplete
        assert_eq!(decode_i_am(&data).unwrap_err(), DecodeError::IncompleteData);
    }

    #[test]
    fn test_decode_i_am_router_to_network_single() {
        let data = [0x01, 0x00, 0x05]; // message type + net 5
        let msg = decode_i_am_router_to_network(&data).unwrap();
        assert_eq!(msg.networks, vec![5]);
    }

    #[test]
    fn test_decode_i_am_router_to_network_multiple() {
        let data = [0x01, 0x00, 0x05, 0x00, 0x0A, 0xAB, 0xCD];
        let msg = decode_i_am_router_to_network(&data).unwrap();
        assert_eq!(msg.networks, vec![5, 10, 0xABCD]);
    }

    #[test]
    fn test_decode_i_am_router_wrong_message_type() {
        let data = [0x00, 0x00, 0x05];
        assert_eq!(
            decode_i_am_router_to_network(&data).unwrap_err(),
            DecodeError::InvalidData
        );
    }

    #[test]
    fn test_decode_i_am_router_odd_length() {
        let data = [0x01, 0x00, 0x05, 0x00]; // 3 network bytes — odd, invalid
        assert_eq!(
            decode_i_am_router_to_network(&data).unwrap_err(),
            DecodeError::InvalidData
        );
    }

    #[test]
    fn test_decode_i_am_router_empty_networks() {
        // Valid: router knows of zero networks
        let data = [0x01];
        let msg = decode_i_am_router_to_network(&data).unwrap();
        assert!(msg.networks.is_empty());
    }
}
