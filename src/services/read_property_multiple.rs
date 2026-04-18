//! ReadPropertyMultiple service — request encoder and response decoder.
//!
//! Service choice: 14

use crate::codec::types::{DecodeError, ObjectIdentifier, PropertyValue};
use crate::enums::PropertyIdentifier;
use crate::services::{
    decode_context_object_id, decode_context_unsigned, encode_closing_tag,
    encode_context_object_id, encode_context_unsigned, encode_opening_tag, is_closing_tag,
    is_opening_tag,
};

pub const SERVICE_CHOICE: u8 = 14;

// ─────────────────────────────────────────────────────────────────────────────
// Request encoder
// ─────────────────────────────────────────────────────────────────────────────

/// A single property reference within a ReadPropertyMultiple request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropertyReference {
    pub property_id: PropertyIdentifier,
    pub array_index: Option<u32>,
}

/// One entry in a ReadPropertyMultiple request.
#[derive(Debug, Clone)]
pub struct ReadAccessSpec {
    pub object_id: ObjectIdentifier,
    pub properties: Vec<PropertyReference>,
}

/// Encode a ReadPropertyMultiple-Request service data.
///
/// For each entry:
///   context[0] object-identifier
///   context[1] opening + { context[0] prop-id, context[1] array-index? }* + closing
pub fn encode_request(specs: &[ReadAccessSpec]) -> Vec<u8> {
    let mut buf = Vec::new();
    for spec in specs {
        encode_context_object_id(0, spec.object_id.to_u32(), &mut buf);
        encode_opening_tag(1, &mut buf);
        for prop in &spec.properties {
            encode_context_unsigned(0, u32::from(prop.property_id), &mut buf);
            if let Some(idx) = prop.array_index {
                encode_context_unsigned(1, idx, &mut buf);
            }
        }
        encode_closing_tag(1, &mut buf);
    }
    buf
}

// ─────────────────────────────────────────────────────────────────────────────
// Response decoder
// ─────────────────────────────────────────────────────────────────────────────

/// A BACnet service-level error (error class + error code).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BacnetError {
    pub error_class: u32,
    pub error_code: u32,
}

/// Result for a single property within ReadPropertyMultiple.
#[derive(Debug, Clone, PartialEq)]
pub struct PropertyResult {
    pub property_id: PropertyIdentifier,
    pub array_index: Option<u32>,
    pub value: Result<PropertyValue, BacnetError>,
}

/// Result for one object in a ReadPropertyMultiple response.
#[derive(Debug, Clone, PartialEq)]
pub struct ObjectResult {
    pub object_id: ObjectIdentifier,
    pub properties: Vec<PropertyResult>,
}

/// Full ReadPropertyMultiple response.
#[derive(Debug, Clone, PartialEq)]
pub struct ReadPropertyMultipleResult {
    pub objects: Vec<ObjectResult>,
}

/// Decode ReadPropertyMultiple ComplexACK service data.
///
/// Layout (repeated per object):
///   context[0] object-identifier
///   context[1] opening
///     (repeated per property):
///       context[2] opening
///         context[0] property-identifier
///         context[1] array-index (optional)
///         context[4] opening + PropertyValue + closing   (success)
///         context[5] opening + error-class + error-code + closing  (error)
///       context[2] closing
///   context[1] closing
pub fn decode_response(data: &[u8]) -> Result<ReadPropertyMultipleResult, DecodeError> {
    let mut offset = 0;
    let mut objects = Vec::new();

    while offset < data.len() {
        let (oid_raw, n) = decode_context_object_id(&data[offset..], 0)?;
        offset += n;
        let object_id = ObjectIdentifier::from_u32(oid_raw);

        if !is_opening_tag(&data[offset..], 1) {
            return Err(DecodeError::InvalidContextTag);
        }
        offset += 1; // opening [1]

        let mut properties = Vec::new();

        while !is_closing_tag(&data[offset..], 1) {
            if offset >= data.len() {
                return Err(DecodeError::IncompleteData);
            }

            // Some devices wrap each property result in [2] open/close (conformant).
            // Others encode the prop-id directly as context[2] without the wrapper
            // (non-conformant but common). Accept both.
            let use_wrapper = is_opening_tag(&data[offset..], 2);
            if use_wrapper {
                offset += 1; // opening [2]
            }

            let (property_id_raw, n) = if use_wrapper {
                decode_context_unsigned(&data[offset..], 0)?
            } else {
                decode_context_unsigned(&data[offset..], 2)?
            };
            offset += n;
            let property_id = PropertyIdentifier::from(property_id_raw);

            let array_index = if offset < data.len() && {
                let b = data[offset];
                let tag_n = (b >> 4) & 0x0F;
                let is_ctx = (b & 0x08) != 0;
                let not_open_close = (b & 0x07) != 0x06 && (b & 0x07) != 0x07;
                is_ctx && not_open_close && tag_n == if use_wrapper { 1 } else { 3 }
            } {
                let ai_tag = if use_wrapper { 1 } else { 3 };
                let (idx, n) = decode_context_unsigned(&data[offset..], ai_tag)?;
                offset += n;
                Some(idx)
            } else {
                None
            };

            let value = if is_opening_tag(&data[offset..], 4) {
                offset += 1; // opening [4]
                let mut values: Vec<PropertyValue> = Vec::new();
                while !is_closing_tag(&data[offset..], 4) {
                    if offset >= data.len() {
                        return Err(DecodeError::IncompleteData);
                    }
                    let (pv, n) = PropertyValue::decode(&data[offset..])?;
                    offset += n;
                    values.push(pv);
                }
                offset += 1; // closing [4]
                let pv = if values.len() == 1 {
                    values.remove(0)
                } else {
                    PropertyValue::Array(values)
                };
                Ok(pv)
            } else if is_opening_tag(&data[offset..], 5) {
                offset += 1; // opening [5]
                let (error_class, n) = decode_enumerated_value(&data[offset..])?;
                offset += n;
                let (error_code, n) = decode_enumerated_value(&data[offset..])?;
                offset += n;
                if !is_closing_tag(&data[offset..], 5) {
                    return Err(DecodeError::InvalidContextTag);
                }
                offset += 1; // closing [5]
                Err(BacnetError {
                    error_class,
                    error_code,
                })
            } else {
                return Err(DecodeError::InvalidContextTag);
            };

            // closing [2] only if we opened it
            if use_wrapper {
                if !is_closing_tag(&data[offset..], 2) {
                    return Err(DecodeError::InvalidContextTag);
                }
                offset += 1;
            }

            properties.push(PropertyResult {
                property_id,
                array_index,
                value,
            });
        }
        offset += 1; // closing [1]

        objects.push(ObjectResult {
            object_id,
            properties,
        });
    }

    Ok(ReadPropertyMultipleResult { objects })
}

/// Decode a BACnet Enumerated (app tag 9) value. Returns (value, consumed).
fn decode_enumerated_value(data: &[u8]) -> Result<(u32, usize), DecodeError> {
    if data.is_empty() {
        return Err(DecodeError::IncompleteData);
    }
    let tag = data[0];
    if (tag >> 4) != 9 {
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
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::types::ObjectType;

    fn device_1() -> ObjectIdentifier {
        ObjectIdentifier::new(ObjectType::Device, 1)
    }
    fn ai_0() -> ObjectIdentifier {
        ObjectIdentifier::new(ObjectType::AnalogInput, 0)
    }

    #[test]
    fn test_encode_request_single_object() {
        let specs = vec![ReadAccessSpec {
            object_id: device_1(),
            properties: vec![
                PropertyReference {
                    property_id: PropertyIdentifier::PresentValue,
                    array_index: None,
                },
                PropertyReference {
                    property_id: PropertyIdentifier::ObjectName,
                    array_index: None,
                },
            ],
        }];
        let buf = encode_request(&specs);
        // starts with context[0] object-id tag 0x0C
        assert_eq!(buf[0], 0x0C);
        // opening [1] = 0x1E
        assert_eq!(buf[5], 0x1E);
        // closing [1] = 0x1F somewhere at the end
        assert_eq!(*buf.last().unwrap(), 0x1F);
    }

    #[test]
    fn test_encode_request_multiple_objects() {
        let specs = vec![
            ReadAccessSpec {
                object_id: device_1(),
                properties: vec![PropertyReference {
                    property_id: PropertyIdentifier::PresentValue,
                    array_index: None,
                }],
            },
            ReadAccessSpec {
                object_id: ai_0(),
                properties: vec![PropertyReference {
                    property_id: PropertyIdentifier::PresentValue,
                    array_index: None,
                }],
            },
        ];
        let buf = encode_request(&specs);
        // Should contain two object-id tags (0x0C)
        let count = buf.iter().filter(|&&b| b == 0x0C).count();
        assert_eq!(count, 2);
    }

    #[test]
    fn test_decode_response_all_success() {
        // Build a single-object, single-property success response
        // Device,1 / prop 85 / Real(22.5)
        let mut data = Vec::new();
        encode_context_object_id(0, device_1().to_u32(), &mut data);
        encode_opening_tag(1, &mut data);
        encode_opening_tag(2, &mut data);
        encode_context_unsigned(0, 85, &mut data);
        // value opening [4]
        data.push(0x4E);
        data.push(0x44); // Real app tag
        data.extend_from_slice(&22.5f32.to_be_bytes());
        data.push(0x4F); // closing [4]
        encode_closing_tag(2, &mut data);
        encode_closing_tag(1, &mut data);

        let result = decode_response(&data).unwrap();
        assert_eq!(result.objects.len(), 1);
        let obj = &result.objects[0];
        assert_eq!(obj.object_id, device_1());
        assert_eq!(obj.properties.len(), 1);
        assert_eq!(
            obj.properties[0].property_id,
            PropertyIdentifier::PresentValue
        );
        assert_eq!(obj.properties[0].value, Ok(PropertyValue::Real(22.5)));
    }

    #[test]
    fn test_decode_response_partial_error() {
        // Device,1: prop 85 = Real(1.0), prop 77 = Error(2, 31)
        let mut data = Vec::new();
        encode_context_object_id(0, device_1().to_u32(), &mut data);
        encode_opening_tag(1, &mut data);

        // Property 85 — success
        encode_opening_tag(2, &mut data);
        encode_context_unsigned(0, 85, &mut data);
        data.push(0x4E);
        data.push(0x44);
        data.extend_from_slice(&1.0f32.to_be_bytes());
        data.push(0x4F);
        encode_closing_tag(2, &mut data);

        // Property 77 — error
        encode_opening_tag(2, &mut data);
        encode_context_unsigned(0, 77, &mut data);
        data.push(0x5E); // opening [5]
        data.push(0x91);
        data.push(0x02); // error-class = 2
        data.push(0x91);
        data.push(0x1F); // error-code = 31
        data.push(0x5F); // closing [5]
        encode_closing_tag(2, &mut data);

        encode_closing_tag(1, &mut data);

        let result = decode_response(&data).unwrap();
        let props = &result.objects[0].properties;
        assert_eq!(props[0].value, Ok(PropertyValue::Real(1.0)));
        assert_eq!(
            props[1].value,
            Err(BacnetError {
                error_class: 2,
                error_code: 31
            })
        );
    }

    #[test]
    fn test_decode_response_two_objects() {
        let mut data = Vec::new();
        for (obj, prop_val) in [(&device_1(), 1.0f32), (&ai_0(), 2.5f32)] {
            encode_context_object_id(0, obj.to_u32(), &mut data);
            encode_opening_tag(1, &mut data);
            encode_opening_tag(2, &mut data);
            encode_context_unsigned(0, 85, &mut data);
            data.push(0x4E);
            data.push(0x44);
            data.extend_from_slice(&prop_val.to_be_bytes());
            data.push(0x4F);
            encode_closing_tag(2, &mut data);
            encode_closing_tag(1, &mut data);
        }
        let result = decode_response(&data).unwrap();
        assert_eq!(result.objects.len(), 2);
        assert_eq!(result.objects[0].object_id, device_1());
        assert_eq!(result.objects[1].object_id, ai_0());
    }
}
