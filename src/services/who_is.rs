//! Who-Is and WhoIsRouterToNetwork service encoders.
//!
//! Who-Is unconfirmed service choice: 8
//! WhoIsRouterToNetwork is a network-layer message (NPDU message type 0x01).

use crate::services::encode_context_unsigned;

pub const WHO_IS_SERVICE_CHOICE: u8 = 8;

/// Network-layer message type for WhoIsRouterToNetwork.
pub const MSG_WHO_IS_ROUTER_TO_NETWORK: u8 = 0x00;

// ─────────────────────────────────────────────────────────────────────────────
// Who-Is encoder
// ─────────────────────────────────────────────────────────────────────────────

/// Encode a Who-Is unconfirmed request APDU service data.
///
/// Without range: empty (global broadcast).
/// With range:
///   context[0] low-limit (unsigned)
///   context[1] high-limit (unsigned)
pub fn encode_who_is(range: Option<(u32, u32)>) -> Vec<u8> {
    let mut buf = Vec::new();
    if let Some((low, high)) = range {
        encode_context_unsigned(0, low, &mut buf);
        encode_context_unsigned(1, high, &mut buf);
    }
    buf
}

// ─────────────────────────────────────────────────────────────────────────────
// WhoIsRouterToNetwork encoder
// ─────────────────────────────────────────────────────────────────────────────

/// Encode a WhoIsRouterToNetwork network-layer message payload.
///
/// All-networks: just the message-type byte (0x00).
/// Specific network: message-type byte + 2-byte network number (big-endian).
pub fn encode_who_is_router_to_network(network: Option<u16>) -> Vec<u8> {
    let mut buf = vec![MSG_WHO_IS_ROUTER_TO_NETWORK];
    if let Some(net) = network {
        buf.push((net >> 8) as u8);
        buf.push(net as u8);
    }
    buf
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_who_is_no_range() {
        let buf = encode_who_is(None);
        assert!(buf.is_empty());
    }

    #[test]
    fn test_who_is_with_range() {
        // range 0..=4194302
        let buf = encode_who_is(Some((0, 4_194_302)));
        // context[0] low=0: tag=0x09 (tag0, ctx, len1), 0x00
        assert_eq!(buf[0], 0x09);
        assert_eq!(buf[1], 0x00);
        // context[1] high=4194302: tag1, ctx, len 3 bytes (0x3F FFFE)
        assert_eq!(buf[2], 0x1B); // tag1, context, len3
        assert_eq!(buf[3], 0x3F);
        assert_eq!(buf[4], 0xFF);
        assert_eq!(buf[5], 0xFE);
    }

    #[test]
    fn test_who_is_with_small_range() {
        let buf = encode_who_is(Some((10, 20)));
        assert_eq!(buf[0], 0x09); // tag0, ctx, len1
        assert_eq!(buf[1], 10);
        assert_eq!(buf[2], 0x19); // tag1, ctx, len1
        assert_eq!(buf[3], 20);
    }

    #[test]
    fn test_who_is_router_to_network_all() {
        let buf = encode_who_is_router_to_network(None);
        assert_eq!(buf, [0x00]);
    }

    #[test]
    fn test_who_is_router_to_network_specific() {
        let buf = encode_who_is_router_to_network(Some(5));
        assert_eq!(buf, [0x00, 0x00, 0x05]);
    }

    #[test]
    fn test_who_is_router_to_network_large() {
        let buf = encode_who_is_router_to_network(Some(0xABCD));
        assert_eq!(buf, [0x00, 0xAB, 0xCD]);
    }
}
