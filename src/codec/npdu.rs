use crate::codec::types::DecodeError;

// NPDU control octet bit masks (byte 1)
const CTRL_NETWORK_LAYER_MSG: u8 = 0x80; // message is a network-layer message (no APDU)
const CTRL_DNET_PRESENT: u8 = 0x20; // destination specifier present
const CTRL_SNET_PRESENT: u8 = 0x08; // source specifier present
const CTRL_DATA_EXPECTING_REPLY: u8 = 0x04;
// priority occupies bits 1-0
const PRIORITY_MASK: u8 = 0x03;

pub const NPDU_VERSION: u8 = 0x01;

/// Message priority (ASHRAE 135 §6.2.2)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Priority {
    #[default]
    Normal = 0,
    Urgent = 1,
    CriticalEquipment = 2,
    LifeSafety = 3,
}

impl Priority {
    pub fn from_u8(v: u8) -> Self {
        match v & PRIORITY_MASK {
            0 => Priority::Normal,
            1 => Priority::Urgent,
            2 => Priority::CriticalEquipment,
            _ => Priority::LifeSafety,
        }
    }

    pub fn to_u8(self) -> u8 {
        self as u8
    }
}

/// A BACnet network address (network number + MAC-layer address bytes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkAddress {
    /// Network number (0 = local, 0xFFFF = broadcast).
    pub net: u16,
    /// MAC-layer address bytes (0 bytes = broadcast on that network).
    pub adr: Vec<u8>,
}

impl NetworkAddress {
    pub fn new(net: u16, adr: Vec<u8>) -> Self {
        Self { net, adr }
    }
}

/// Parameters for encoding an NPDU.
#[derive(Debug, Clone, Default)]
pub struct NpduEncodeParams<'a> {
    /// APDU (or network-layer message) payload to wrap.
    pub apdu: &'a [u8],
    /// True when a reply is expected (sets the data-expecting-reply bit).
    pub data_expecting_reply: bool,
    /// Message priority.
    pub priority: Priority,
    /// Optional destination network address. When present the hop count is
    /// included in the encoded frame.
    pub dest: Option<NetworkAddress>,
    /// Optional source network address.
    pub src: Option<NetworkAddress>,
    /// Hop count written when `dest` is present. Defaults to 255.
    pub hop_count: Option<u8>,
    /// True when the payload is a network-layer message (sets CTRL bit 7).
    pub is_network_layer_message: bool,
}

/// Encode an NPDU frame.
///
/// Layout:
///   [0] version = 0x01
///   [1] control octet
///   [if DNET] 2-byte DNET, 1-byte DADR-len, DADR bytes, 1-byte hop-count
///   [if SNET] 2-byte SNET, 1-byte SADR-len, SADR bytes
///   [...] APDU bytes
pub fn encode(params: &NpduEncodeParams<'_>) -> Vec<u8> {
    let mut ctrl: u8 = params.priority.to_u8();
    if params.data_expecting_reply {
        ctrl |= CTRL_DATA_EXPECTING_REPLY;
    }
    if params.dest.is_some() {
        ctrl |= CTRL_DNET_PRESENT;
    }
    if params.src.is_some() {
        ctrl |= CTRL_SNET_PRESENT;
    }
    if params.is_network_layer_message {
        ctrl |= CTRL_NETWORK_LAYER_MSG;
    }

    let mut buf = Vec::new();
    buf.push(NPDU_VERSION);
    buf.push(ctrl);

    if let Some(dest) = &params.dest {
        buf.push((dest.net >> 8) as u8);
        buf.push(dest.net as u8);
        buf.push(dest.adr.len() as u8);
        buf.extend_from_slice(&dest.adr);
        buf.push(params.hop_count.unwrap_or(255));
    }

    if let Some(src) = &params.src {
        buf.push((src.net >> 8) as u8);
        buf.push(src.net as u8);
        buf.push(src.adr.len() as u8);
        buf.extend_from_slice(&src.adr);
    }

    buf.extend_from_slice(params.apdu);
    buf
}

/// Decoded fields from an NPDU header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NpduFrame<'a> {
    pub priority: Priority,
    pub data_expecting_reply: bool,
    pub is_network_layer_message: bool,
    pub dest: Option<NetworkAddress>,
    pub src: Option<NetworkAddress>,
    /// Hop count — only present when `dest` is `Some`.
    pub hop_count: Option<u8>,
    /// The inner APDU (or network-layer message) bytes.
    pub apdu: &'a [u8],
}

/// Decode an NPDU byte sequence.
///
/// Returns an `NpduFrame` with parsed fields and a slice into the inner APDU.
pub fn decode(data: &[u8]) -> Result<NpduFrame<'_>, DecodeError> {
    if data.len() < 2 {
        return Err(DecodeError::IncompleteData);
    }
    if data[0] != NPDU_VERSION {
        return Err(DecodeError::InvalidData);
    }

    let ctrl = data[1];
    let priority = Priority::from_u8(ctrl);
    let data_expecting_reply = (ctrl & CTRL_DATA_EXPECTING_REPLY) != 0;
    let is_network_layer_message = (ctrl & CTRL_NETWORK_LAYER_MSG) != 0;
    let has_dnet = (ctrl & CTRL_DNET_PRESENT) != 0;
    let has_snet = (ctrl & CTRL_SNET_PRESENT) != 0;

    let mut offset = 2usize;

    // Destination specifier
    let (dest, hop_count) = if has_dnet {
        if data.len() < offset + 3 {
            return Err(DecodeError::IncompleteData);
        }
        let net = ((data[offset] as u16) << 8) | data[offset + 1] as u16;
        offset += 2;
        let adr_len = data[offset] as usize;
        offset += 1;
        if data.len() < offset + adr_len + 1 {
            return Err(DecodeError::IncompleteData);
        }
        let adr = data[offset..offset + adr_len].to_vec();
        offset += adr_len;
        let hop = data[offset];
        offset += 1;
        (Some(NetworkAddress { net, adr }), Some(hop))
    } else {
        (None, None)
    };

    // Source specifier
    let src = if has_snet {
        if data.len() < offset + 3 {
            return Err(DecodeError::IncompleteData);
        }
        let net = ((data[offset] as u16) << 8) | data[offset + 1] as u16;
        offset += 2;
        let adr_len = data[offset] as usize;
        offset += 1;
        if data.len() < offset + adr_len {
            return Err(DecodeError::IncompleteData);
        }
        let adr = data[offset..offset + adr_len].to_vec();
        offset += adr_len;
        Some(NetworkAddress { net, adr })
    } else {
        None
    };

    Ok(NpduFrame {
        priority,
        data_expecting_reply,
        is_network_layer_message,
        dest,
        src,
        hop_count,
        apdu: &data[offset..],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal local NPDU (no routing) with data-expecting-reply set.
    #[test]
    fn test_encode_no_routing_data_expecting_reply() {
        let apdu = [0x00, 0x01, 0x02];
        let params = NpduEncodeParams {
            apdu: &apdu,
            data_expecting_reply: true,
            priority: Priority::Normal,
            ..Default::default()
        };
        let frame = encode(&params);
        // version=0x01, ctrl=0x04 (data_expecting_reply), then APDU
        assert_eq!(frame, [0x01, 0x04, 0x00, 0x01, 0x02]);
    }

    /// NPDU without data-expecting-reply.
    #[test]
    fn test_encode_no_routing_no_reply() {
        let apdu = [0xAB];
        let params = NpduEncodeParams {
            apdu: &apdu,
            ..Default::default()
        };
        let frame = encode(&params);
        assert_eq!(frame, [0x01, 0x00, 0xAB]);
    }

    /// NPDU with destination network address (DNET present).
    #[test]
    fn test_encode_with_dest() {
        let apdu = [0xFF];
        let dest = NetworkAddress::new(5, vec![0x0A]); // net=5, 1-byte adr
        let params = NpduEncodeParams {
            apdu: &apdu,
            data_expecting_reply: true,
            dest: Some(dest),
            hop_count: Some(10),
            ..Default::default()
        };
        let frame = encode(&params);
        // version=0x01
        // ctrl=0x24 (DNET_PRESENT | data_expecting_reply)
        // DNET: 0x00 0x05
        // DADR-len: 0x01
        // DADR: 0x0A
        // hop-count: 0x0A
        // APDU: 0xFF
        assert_eq!(frame, [0x01, 0x24, 0x00, 0x05, 0x01, 0x0A, 0x0A, 0xFF]);
    }

    /// NPDU with broadcast destination (DADR-len = 0).
    #[test]
    fn test_encode_broadcast_dest() {
        let apdu = [0x10];
        let dest = NetworkAddress::new(0xFFFF, vec![]); // global broadcast
        let params = NpduEncodeParams {
            apdu: &apdu,
            dest: Some(dest),
            ..Default::default()
        };
        let frame = encode(&params);
        // ctrl=0x20 (DNET_PRESENT)
        // DNET: 0xFF 0xFF, DADR-len: 0x00, hop-count: 0xFF (default)
        assert_eq!(frame, [0x01, 0x20, 0xFF, 0xFF, 0x00, 0xFF, 0x10]);
    }

    /// Priority bits are encoded correctly.
    #[test]
    fn test_encode_priority_life_safety() {
        let params = NpduEncodeParams {
            apdu: &[],
            priority: Priority::LifeSafety,
            ..Default::default()
        };
        let frame = encode(&params);
        assert_eq!(frame[1] & 0x03, 0x03); // bits 1-0 = 11
    }

    /// Decode a minimal NPDU (no routing).
    #[test]
    fn test_decode_no_routing() {
        let data = [0x01, 0x04, 0x00, 0x01];
        let frame = decode(&data).unwrap();
        assert_eq!(frame.priority, Priority::Normal);
        assert!(frame.data_expecting_reply);
        assert!(!frame.is_network_layer_message);
        assert!(frame.dest.is_none());
        assert!(frame.src.is_none());
        assert!(frame.hop_count.is_none());
        assert_eq!(frame.apdu, &[0x00, 0x01]);
    }

    /// Decode an NPDU with DNET present.
    #[test]
    fn test_decode_with_dest() {
        // version, ctrl(DNET_PRESENT|DER), DNET=5, DADR-len=1, DADR=0x0A, hop=10, APDU=0xFF
        let data = [0x01, 0x24, 0x00, 0x05, 0x01, 0x0A, 0x0A, 0xFF];
        let frame = decode(&data).unwrap();
        assert_eq!(frame.priority, Priority::Normal);
        assert!(frame.data_expecting_reply);
        let dest = frame.dest.unwrap();
        assert_eq!(dest.net, 5);
        assert_eq!(dest.adr, vec![0x0A]);
        assert_eq!(frame.hop_count, Some(10));
        assert_eq!(frame.apdu, &[0xFF]);
    }

    /// Decode an NPDU with source specifier.
    #[test]
    fn test_decode_with_src() {
        // version, ctrl(SNET_PRESENT), SNET=3, SADR-len=1, SADR=0xBB, APDU=0x01
        let data = [0x01, 0x08, 0x00, 0x03, 0x01, 0xBB, 0x01];
        let frame = decode(&data).unwrap();
        let src = frame.src.unwrap();
        assert_eq!(src.net, 3);
        assert_eq!(src.adr, vec![0xBB]);
        assert_eq!(frame.apdu, &[0x01]);
    }

    /// Invalid version byte returns error.
    #[test]
    fn test_decode_wrong_version() {
        let data = [0x02, 0x00];
        assert_eq!(decode(&data).unwrap_err(), DecodeError::InvalidData);
    }

    /// Truncated data returns IncompleteData.
    #[test]
    fn test_decode_truncated() {
        let data = [0x01];
        assert_eq!(decode(&data).unwrap_err(), DecodeError::IncompleteData);
    }

    /// Round-trip: encode then decode recovers the same fields.
    #[test]
    fn test_roundtrip_no_routing() {
        let apdu = vec![0xDE, 0xAD, 0xBE, 0xEF];
        let params = NpduEncodeParams {
            apdu: &apdu,
            data_expecting_reply: true,
            priority: Priority::Urgent,
            ..Default::default()
        };
        let encoded = encode(&params);
        let frame = decode(&encoded).unwrap();
        assert_eq!(frame.priority, Priority::Urgent);
        assert!(frame.data_expecting_reply);
        assert_eq!(frame.apdu, apdu.as_slice());
    }

    /// Round-trip with destination network address.
    #[test]
    fn test_roundtrip_with_dest() {
        let apdu = vec![0x01, 0x02];
        let dest = NetworkAddress::new(100, vec![0xAA, 0xBB]);
        let params = NpduEncodeParams {
            apdu: &apdu,
            data_expecting_reply: false,
            dest: Some(dest.clone()),
            hop_count: Some(8),
            ..Default::default()
        };
        let encoded = encode(&params);
        let frame = decode(&encoded).unwrap();
        assert_eq!(frame.dest.unwrap(), dest);
        assert_eq!(frame.hop_count, Some(8));
        assert_eq!(frame.apdu, apdu.as_slice());
    }
}
