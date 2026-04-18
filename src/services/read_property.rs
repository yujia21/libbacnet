//! ReadProperty service — request encoder and response decoder.
//!
//! Service choice: 12

use crate::codec::types::{DecodeError, ObjectIdentifier, PropertyValue};
use crate::enums::PropertyIdentifier;
use crate::services::{
    decode_context_object_id, decode_context_unsigned, encode_context_object_id,
    encode_context_unsigned, is_closing_tag, is_opening_tag,
};

pub const SERVICE_CHOICE: u8 = 12;

// ─────────────────────────────────────────────────────────────────────────────
// Request encoder
// ─────────────────────────────────────────────────────────────────────────────

/// Encode a ReadProperty-Request service data.
///
/// Layout:
///   context[0] object-identifier (4 bytes)
///   context[1] property-identifier (unsigned, 1–4 bytes)
///   context[2] array-index (optional, unsigned)
pub fn encode_request(
    object_id: &ObjectIdentifier,
    property_id: PropertyIdentifier,
    array_index: Option<u32>,
) -> Vec<u8> {
    let mut buf = Vec::new();
    encode_context_object_id(0, object_id.to_u32(), &mut buf);
    encode_context_unsigned(1, u32::from(property_id), &mut buf);
    if let Some(idx) = array_index {
        encode_context_unsigned(2, idx, &mut buf);
    }
    buf
}

// ─────────────────────────────────────────────────────────────────────────────
// Response decoder
// ─────────────────────────────────────────────────────────────────────────────

/// Decoded result of a ReadProperty response.
#[derive(Debug, Clone, PartialEq)]
pub struct ReadPropertyResult {
    pub object_id: ObjectIdentifier,
    pub property_id: PropertyIdentifier,
    pub array_index: Option<u32>,
    pub value: PropertyValue,
}

/// Decode ReadProperty ComplexACK service data.
///
/// Layout:
///   context[0] object-identifier
///   context[1] property-identifier
///   context[2] array-index (optional)
///   context[3] opening-tag + PropertyValue + closing-tag
pub fn decode_response(data: &[u8]) -> Result<ReadPropertyResult, DecodeError> {
    let mut offset = 0;

    let (oid_raw, n) = decode_context_object_id(&data[offset..], 0)?;
    offset += n;
    let object_id = ObjectIdentifier::from_u32(oid_raw);

    let (property_id_raw, n) = decode_context_unsigned(&data[offset..], 1)?;
    offset += n;
    let property_id = PropertyIdentifier::from(property_id_raw);

    // Optional array index
    let array_index = if offset < data.len() && {
        let b = data[offset];
        (b >> 4) == 2 && (b & 0x08) != 0
    } {
        let (idx, n) = decode_context_unsigned(&data[offset..], 2)?;
        offset += n;
        Some(idx)
    } else {
        None
    };

    // Opening tag [3]
    if !is_opening_tag(&data[offset..], 3) {
        return Err(DecodeError::InvalidContextTag);
    }
    offset += 1;

    // Collect all PropertyValues until closing tag [3].
    // Scalar properties have exactly one value; array/list properties have many.
    let mut values: Vec<PropertyValue> = Vec::new();
    while !is_closing_tag(&data[offset..], 3) {
        if offset >= data.len() {
            return Err(DecodeError::IncompleteData);
        }
        let (pv, n) = PropertyValue::decode(&data[offset..])?;
        offset += n;
        values.push(pv);
    }

    let value = if values.len() == 1 {
        values.remove(0)
    } else {
        PropertyValue::Array(values)
    };

    // Closing tag [3]
    if !is_closing_tag(&data[offset..], 3) {
        return Err(DecodeError::InvalidContextTag);
    }

    Ok(ReadPropertyResult {
        object_id,
        property_id,
        array_index,
        value,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::types::ObjectType;

    fn device_5() -> ObjectIdentifier {
        ObjectIdentifier::new(ObjectType::Device, 5)
    }

    #[test]
    fn test_encode_request_no_array_index() {
        // Device,5 / present-value (prop 85) / no array index
        let buf = encode_request(&device_5(), PropertyIdentifier::PresentValue, None);
        // context[0] object-id: tag=0x0C (tag0, context, len4), then 4 bytes
        assert_eq!(buf[0], 0x0C);
        // context[1] prop-id=85: tag=0x19 (tag1, context, len1), 0x55
        let oid_end = 5;
        assert_eq!(buf[oid_end], 0x19);
        assert_eq!(buf[oid_end + 1], 85);
        assert_eq!(buf.len(), 7); // 5 + 2
    }

    #[test]
    fn test_encode_request_with_array_index() {
        let buf = encode_request(&device_5(), PropertyIdentifier::PresentValue, Some(0));
        // 5 (oid) + 2 (prop) + 2 (array index 0) = 9
        assert_eq!(buf.len(), 9);
        // array-index tag: context tag 2, len 1
        assert_eq!(buf[7], 0x29);
        assert_eq!(buf[8], 0x00);
    }

    #[test]
    fn test_decode_response_real_value() {
        // Build a known-good response manually:
        // context[0] oid = Device,5
        // context[1] prop = 85 (present-value)
        // context[3] opening + Real(1.0) + closing
        let mut data = Vec::new();
        encode_context_object_id(0, device_5().to_u32(), &mut data);
        encode_context_unsigned(1, 85, &mut data);
        // opening tag [3]
        data.push(0x3E);
        // Real(1.0): app tag 4, len 4, 0x3F800000
        data.push(0x44);
        data.extend_from_slice(&1.0f32.to_be_bytes());
        // closing tag [3]
        data.push(0x3F);

        let result = decode_response(&data).unwrap();
        assert_eq!(result.object_id, device_5());
        assert_eq!(result.property_id, PropertyIdentifier::PresentValue);
        assert_eq!(result.array_index, None);
        assert_eq!(result.value, PropertyValue::Real(1.0));
    }

    #[test]
    fn test_decode_response_with_array_index() {
        let mut data = Vec::new();
        encode_context_object_id(0, device_5().to_u32(), &mut data);
        encode_context_unsigned(1, 85, &mut data);
        encode_context_unsigned(2, 3, &mut data); // array index = 3
        data.push(0x3E);
        data.push(0x21); // Unsigned(1): tag2, len1
        data.push(0x01);
        data.push(0x3F);

        let result = decode_response(&data).unwrap();
        assert_eq!(result.array_index, Some(3));
        assert_eq!(result.value, PropertyValue::Unsigned(1));
    }

    #[test]
    fn test_roundtrip_request_fields() {
        // encode request, check service data fields are recoverable
        let buf = encode_request(&device_5(), PropertyIdentifier::PresentValue, None);
        // first 5 bytes = context oid
        let (oid_raw, n) = decode_context_object_id(&buf, 0).unwrap();
        assert_eq!(ObjectIdentifier::from_u32(oid_raw), device_5());
        let (prop, _) = decode_context_unsigned(&buf[n..], 1).unwrap();
        assert_eq!(prop, 85);
    }
}
