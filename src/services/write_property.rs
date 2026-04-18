//! WriteProperty service — request encoder.
//!
//! Service choice: 15

use crate::codec::types::{ObjectIdentifier, PropertyValue};
use crate::enums::PropertyIdentifier;
use crate::services::{
    encode_closing_tag, encode_context_object_id, encode_context_unsigned, encode_opening_tag,
};

pub const SERVICE_CHOICE: u8 = 15;

// ─────────────────────────────────────────────────────────────────────────────
// Request encoder
// ─────────────────────────────────────────────────────────────────────────────

/// Encode a WriteProperty-Request service data.
///
/// Layout:
///   context[0] object-identifier
///   context[1] property-identifier
///   context[2] array-index (optional)
///   context[3] opening + PropertyValue + closing
///   context[4] priority (optional, 1–16)
pub fn encode_request(
    object_id: &ObjectIdentifier,
    property_id: PropertyIdentifier,
    value: &PropertyValue,
    array_index: Option<u32>,
    priority: Option<u8>,
) -> Vec<u8> {
    let mut buf = Vec::new();
    encode_context_object_id(0, object_id.to_u32(), &mut buf);
    encode_context_unsigned(1, u32::from(property_id), &mut buf);
    if let Some(idx) = array_index {
        encode_context_unsigned(2, idx, &mut buf);
    }
    encode_opening_tag(3, &mut buf);
    value.encode_tag_and_value(&mut buf);
    encode_closing_tag(3, &mut buf);
    if let Some(p) = priority {
        encode_context_unsigned(4, p as u32, &mut buf);
    }
    buf
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::types::ObjectType;
    use crate::services::{decode_context_object_id, decode_context_unsigned, is_opening_tag};

    fn ai_1() -> ObjectIdentifier {
        ObjectIdentifier::new(ObjectType::AnalogOutput, 1)
    }

    #[test]
    fn test_encode_write_property_real_no_priority() {
        let buf = encode_request(
            &ai_1(),
            PropertyIdentifier::PresentValue,
            &PropertyValue::Real(42.0),
            None,
            None,
        );
        // context[0] object-id
        let (oid_raw, n) = decode_context_object_id(&buf, 0).unwrap();
        assert_eq!(ObjectIdentifier::from_u32(oid_raw), ai_1());
        // context[1] prop-id
        let (prop, n2) = decode_context_unsigned(&buf[n..], 1).unwrap();
        assert_eq!(prop, 85);
        // opening [3]
        assert!(is_opening_tag(&buf[n + n2..], 3));
        // No priority at the end (closing [3] is last meaningful byte)
        let close_pos = n + n2 + 1 + 1 + 4; // open + Real tag (1) + Real val (4)
        assert_eq!(buf[close_pos], 0x3F); // closing [3]
        assert_eq!(buf.len(), close_pos + 1);
    }

    #[test]
    fn test_encode_write_property_with_priority() {
        let buf = encode_request(
            &ai_1(),
            PropertyIdentifier::PresentValue,
            &PropertyValue::Real(0.0),
            None,
            Some(8),
        );
        // last 2 bytes: context[4] priority = 8
        let len = buf.len();
        assert_eq!(buf[len - 2], 0x49); // tag4, context, len1
        assert_eq!(buf[len - 1], 8);
    }

    #[test]
    fn test_encode_write_property_with_array_index() {
        let buf = encode_request(
            &ai_1(),
            PropertyIdentifier::PresentValue,
            &PropertyValue::Unsigned(1),
            Some(0),
            None,
        );
        // array-index context[2] appears between prop-id and opening[3]
        let (_, n1) = decode_context_object_id(&buf, 0).unwrap();
        let (_, n2) = decode_context_unsigned(&buf[n1..], 1).unwrap();
        // next should be context[2]
        assert_eq!(buf[n1 + n2], 0x29); // tag2, context, len1
        assert_eq!(buf[n1 + n2 + 1], 0x00); // array index = 0
    }

    #[test]
    fn test_encode_write_property_unsigned_value() {
        let buf = encode_request(
            &ai_1(),
            PropertyIdentifier::PresentValue,
            &PropertyValue::Unsigned(100),
            None,
            None,
        );
        // after context[0] + context[1] + opening[3]:
        let (_, n1) = decode_context_object_id(&buf, 0).unwrap();
        let (_, n2) = decode_context_unsigned(&buf[n1..], 1).unwrap();
        let after_open = n1 + n2 + 1;
        // Unsigned(100): tag=0x21 (app tag 2, len 1), value=100
        assert_eq!(buf[after_open], 0x21);
        assert_eq!(buf[after_open + 1], 100);
    }
}
