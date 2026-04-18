pub mod i_am;
pub mod read_property;
pub mod read_property_multiple;
pub mod who_is;
pub mod write_property;

use crate::codec::types::DecodeError;

// ─────────────────────────────────────────────────────────────────────────────
// Shared helpers for BACnet context-tag encoding
// ─────────────────────────────────────────────────────────────────────────────

/// Encode a context-tagged unsigned integer (for property ID, array index, etc.)
/// Uses the minimum byte width needed.
pub(crate) fn encode_context_unsigned(tag_number: u8, value: u32, buf: &mut Vec<u8>) {
    let (len, bytes) = unsigned_to_bytes(value);
    buf.push((tag_number << 4) | 0x08 | len);
    // bytes is big-endian 4 bytes; the significant `len` bytes are at the end
    buf.extend_from_slice(&bytes[4 - len as usize..]);
}

/// Encode a context-tagged object identifier (4 bytes).
pub(crate) fn encode_context_object_id(tag_number: u8, value: u32, buf: &mut Vec<u8>) {
    buf.push((tag_number << 4) | 0x08 | 4u8);
    buf.push((value >> 24) as u8);
    buf.push((value >> 16) as u8);
    buf.push((value >> 8) as u8);
    buf.push(value as u8);
}

/// Opening tag for a constructed context tag.
pub(crate) fn encode_opening_tag(tag_number: u8, buf: &mut Vec<u8>) {
    buf.push((tag_number << 4) | 0x0E);
}

/// Closing tag for a constructed context tag.
pub(crate) fn encode_closing_tag(tag_number: u8, buf: &mut Vec<u8>) {
    buf.push((tag_number << 4) | 0x0F);
}

/// Returns (length_byte, 4-byte big-endian buffer) for minimum-width unsigned encoding.
fn unsigned_to_bytes(value: u32) -> (u8, [u8; 4]) {
    let bytes = value.to_be_bytes();
    let len = if value <= 0xFF {
        1u8
    } else if value <= 0xFFFF {
        2
    } else if value <= 0xFFFFFF {
        3
    } else {
        4
    };
    (len, bytes)
}

/// Decode a context-tagged unsigned integer. Returns (value, bytes_consumed).
pub(crate) fn decode_context_unsigned(
    data: &[u8],
    expected_tag: u8,
) -> Result<(u32, usize), DecodeError> {
    if data.is_empty() {
        return Err(DecodeError::IncompleteData);
    }
    let tag_byte = data[0];
    let tag_number = (tag_byte >> 4) & 0x0F;
    let is_context = (tag_byte & 0x08) != 0;
    if !is_context || tag_number != expected_tag {
        return Err(DecodeError::InvalidContextTag);
    }
    let len = (tag_byte & 0x07) as usize;
    if data.len() < 1 + len {
        return Err(DecodeError::IncompleteData);
    }
    let mut value = 0u32;
    for i in 0..len {
        value = (value << 8) | data[1 + i] as u32;
    }
    Ok((value, 1 + len))
}

/// Decode a context-tagged object identifier (always 4 bytes). Returns (raw_u32, bytes_consumed).
pub(crate) fn decode_context_object_id(
    data: &[u8],
    expected_tag: u8,
) -> Result<(u32, usize), DecodeError> {
    if data.is_empty() {
        return Err(DecodeError::IncompleteData);
    }
    let tag_byte = data[0];
    let tag_number = (tag_byte >> 4) & 0x0F;
    let is_context = (tag_byte & 0x08) != 0;
    let len = (tag_byte & 0x07) as usize;
    if !is_context || tag_number != expected_tag || len != 4 {
        return Err(DecodeError::InvalidContextTag);
    }
    if data.len() < 5 {
        return Err(DecodeError::IncompleteData);
    }
    let value = ((data[1] as u32) << 24)
        | ((data[2] as u32) << 16)
        | ((data[3] as u32) << 8)
        | (data[4] as u32);
    Ok((value, 5))
}

/// Check whether the next byte is an opening tag with the given tag number.
pub(crate) fn is_opening_tag(data: &[u8], tag_number: u8) -> bool {
    !data.is_empty() && data[0] == (tag_number << 4) | 0x0E
}

/// Check whether the next byte is a closing tag with the given tag number.
pub(crate) fn is_closing_tag(data: &[u8], tag_number: u8) -> bool {
    !data.is_empty() && data[0] == (tag_number << 4) | 0x0F
}
