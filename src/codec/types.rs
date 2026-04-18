use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum PropertyValue {
    Null,
    Boolean(bool),
    Unsigned(u32),
    Signed(i32),
    Real(f32),
    Double(f64),
    OctetString(Vec<u8>),
    CharacterString(CharacterString),
    BitString(BitString),
    Enumerated(u32),
    Date(Date),
    Time(Time),
    ObjectIdentifier(ObjectIdentifier),
    /// Multiple decoded values from a ReadProperty array/list response.
    Array(Vec<PropertyValue>),
    Any(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct CharacterString {
    pub encoding: CharacterEncoding,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CharacterEncoding {
    Utf8,
    MicrosoftAnsi,
    Iso8859_1,
    Iso8859_2,
    Iso8859_3,
    Iso8859_4,
    Iso8859_5,
    Iso8859_6,
    Iso8859_7,
    Iso8859_8,
    Iso8859_9,
    Iso8859_10,
    Iso88592_1,
    Utf16,
    Reserved,
}

impl CharacterEncoding {
    pub fn from_u8(value: u8) -> Self {
        match value {
            0 => CharacterEncoding::Utf8,
            1 => CharacterEncoding::MicrosoftAnsi,
            2 => CharacterEncoding::Iso8859_1,
            3 => CharacterEncoding::Iso8859_2,
            4 => CharacterEncoding::Iso8859_3,
            5 => CharacterEncoding::Iso8859_4,
            6 => CharacterEncoding::Iso8859_5,
            7 => CharacterEncoding::Iso8859_6,
            8 => CharacterEncoding::Iso8859_7,
            9 => CharacterEncoding::Iso8859_8,
            10 => CharacterEncoding::Iso8859_9,
            11 => CharacterEncoding::Iso8859_10,
            12 => CharacterEncoding::Iso88592_1,
            13 => CharacterEncoding::Utf16,
            _ => CharacterEncoding::Reserved,
        }
    }

    pub fn to_u8(&self) -> u8 {
        match self {
            CharacterEncoding::Utf8 => 0,
            CharacterEncoding::MicrosoftAnsi => 1,
            CharacterEncoding::Iso8859_1 => 2,
            CharacterEncoding::Iso8859_2 => 3,
            CharacterEncoding::Iso8859_3 => 4,
            CharacterEncoding::Iso8859_4 => 5,
            CharacterEncoding::Iso8859_5 => 6,
            CharacterEncoding::Iso8859_6 => 7,
            CharacterEncoding::Iso8859_7 => 8,
            CharacterEncoding::Iso8859_8 => 9,
            CharacterEncoding::Iso8859_9 => 10,
            CharacterEncoding::Iso8859_10 => 11,
            CharacterEncoding::Iso88592_1 => 12,
            CharacterEncoding::Utf16 => 13,
            CharacterEncoding::Reserved => 14,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct BitString {
    pub used_bits: u8,
    pub bits: Vec<u8>,
}

impl BitString {
    pub fn new(used_bits: u8, bits: Vec<u8>) -> Self {
        Self { used_bits, bits }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Date {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub weekday: Option<Weekday>,
}

impl Date {
    pub fn new(year: u16, month: u8, day: u8, weekday: Option<Weekday>) -> Self {
        Self {
            year,
            month,
            day,
            weekday,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Weekday {
    Monday = 0,
    Tuesday = 1,
    Wednesday = 2,
    Thursday = 3,
    Friday = 4,
    Saturday = 5,
    Sunday = 6,
    MondayToFriday = 7,
}

impl Weekday {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Weekday::Monday),
            1 => Some(Weekday::Tuesday),
            2 => Some(Weekday::Wednesday),
            3 => Some(Weekday::Thursday),
            4 => Some(Weekday::Friday),
            5 => Some(Weekday::Saturday),
            6 => Some(Weekday::Sunday),
            7 => Some(Weekday::MondayToFriday),
            _ => None,
        }
    }

    pub fn to_u8(&self) -> u8 {
        match self {
            Weekday::Monday => 0,
            Weekday::Tuesday => 1,
            Weekday::Wednesday => 2,
            Weekday::Thursday => 3,
            Weekday::Friday => 4,
            Weekday::Saturday => 5,
            Weekday::Sunday => 6,
            Weekday::MondayToFriday => 7,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Time {
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub hundredths: u8,
}

impl Time {
    pub fn new(hour: u8, minute: u8, second: u8, hundredths: u8) -> Self {
        Self {
            hour,
            minute,
            second,
            hundredths,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectIdentifier {
    pub object_type: ObjectType,
    pub instance: u32,
}

impl ObjectIdentifier {
    pub fn new(object_type: ObjectType, instance: u32) -> Self {
        Self {
            object_type,
            instance,
        }
    }

    pub fn from_u32(value: u32) -> Self {
        let object_type = ObjectType::from_u16((value >> 22) as u16);
        let instance = value & 0x3FFFFF;
        Self {
            object_type,
            instance,
        }
    }

    pub fn to_u32(&self) -> u32 {
        ((self.object_type.to_u16() as u32) << 22) | (self.instance & 0x3FFFFF)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectType {
    AnalogInput,
    AnalogOutput,
    AnalogValue,
    BinaryInput,
    BinaryOutput,
    BinaryValue,
    Calendar,
    Command,
    Device,
    EventEnrollment,
    File,
    Group,
    Loop,
    MultiStateInput,
    MultiStateOutput,
    MultiStateValue,
    NotificationClass,
    Program,
    Schedule,
    Averaging,
    CharacterStringValue,
    DateTimeValue,
    IntegerValue,
    LargeAnalogValue,
    MultiStateValue2,
    PositiveIntegerValue,
    TimeValue,
    AccessPoint,
    AccessZone,
    AccessZone2,
    AckedSummarization,
    Action,
    AlarmSummary,
    AlarmSummary2,
    Annotation,
    Attachment,
    AuthParam,
    AuthPolicy,
    Cred,
    CredentialDataInput,
    Door,
    NetworkSecurity,
    NetworkPort,
    ProtocolObject,
    ProtocolServices,
    ProtocolTransaction,
    StructuredRel,
    SecurityKey,
    SecurityKeyset,
    SecurityLevel,
    Service,
    TimePatternValue,
    TimeSeries,
    Unknown(u16),
}

impl ObjectType {
    pub fn from_u16(value: u16) -> Self {
        match value {
            0 => ObjectType::AnalogInput,
            1 => ObjectType::AnalogOutput,
            2 => ObjectType::AnalogValue,
            3 => ObjectType::BinaryInput,
            4 => ObjectType::BinaryOutput,
            5 => ObjectType::BinaryValue,
            6 => ObjectType::Calendar,
            7 => ObjectType::Command,
            8 => ObjectType::Device,
            9 => ObjectType::EventEnrollment,
            10 => ObjectType::File,
            11 => ObjectType::Group,
            12 => ObjectType::Loop,
            13 => ObjectType::MultiStateInput,
            14 => ObjectType::MultiStateOutput,
            15 => ObjectType::MultiStateValue,
            16 => ObjectType::NotificationClass,
            17 => ObjectType::Program,
            18 => ObjectType::Schedule,
            19 => ObjectType::Averaging,
            40 => ObjectType::CharacterStringValue,
            41 => ObjectType::DateTimeValue,
            42 => ObjectType::IntegerValue,
            43 => ObjectType::LargeAnalogValue,
            44 => ObjectType::MultiStateValue2,
            45 => ObjectType::PositiveIntegerValue,
            46 => ObjectType::TimeValue,
            1001 => ObjectType::AccessPoint,
            1002 => ObjectType::AccessZone,
            1003 => ObjectType::AccessZone2,
            1004 => ObjectType::AckedSummarization,
            1005 => ObjectType::Action,
            1006 => ObjectType::AlarmSummary,
            1007 => ObjectType::AlarmSummary2,
            1008 => ObjectType::Annotation,
            1009 => ObjectType::Attachment,
            1010 => ObjectType::AuthParam,
            1011 => ObjectType::AuthPolicy,
            1012 => ObjectType::Cred,
            1013 => ObjectType::CredentialDataInput,
            1014 => ObjectType::Door,
            1015 => ObjectType::NetworkSecurity,
            1016 => ObjectType::NetworkPort,
            1017 => ObjectType::ProtocolObject,
            1018 => ObjectType::ProtocolServices,
            1019 => ObjectType::ProtocolTransaction,
            1020 => ObjectType::StructuredRel,
            1021 => ObjectType::SecurityKey,
            1022 => ObjectType::SecurityKeyset,
            1023 => ObjectType::SecurityLevel,
            1024 => ObjectType::Service,
            1025 => ObjectType::TimePatternValue,
            1026 => ObjectType::TimeSeries,
            _ => ObjectType::Unknown(value),
        }
    }

    pub fn to_u16(&self) -> u16 {
        match self {
            ObjectType::AnalogInput => 0,
            ObjectType::AnalogOutput => 1,
            ObjectType::AnalogValue => 2,
            ObjectType::BinaryInput => 3,
            ObjectType::BinaryOutput => 4,
            ObjectType::BinaryValue => 5,
            ObjectType::Calendar => 6,
            ObjectType::Command => 7,
            ObjectType::Device => 8,
            ObjectType::EventEnrollment => 9,
            ObjectType::File => 10,
            ObjectType::Group => 11,
            ObjectType::Loop => 12,
            ObjectType::MultiStateInput => 13,
            ObjectType::MultiStateOutput => 14,
            ObjectType::MultiStateValue => 15,
            ObjectType::NotificationClass => 16,
            ObjectType::Program => 17,
            ObjectType::Schedule => 18,
            ObjectType::Averaging => 19,
            ObjectType::CharacterStringValue => 40,
            ObjectType::DateTimeValue => 41,
            ObjectType::IntegerValue => 42,
            ObjectType::LargeAnalogValue => 43,
            ObjectType::MultiStateValue2 => 44,
            ObjectType::PositiveIntegerValue => 45,
            ObjectType::TimeValue => 46,
            ObjectType::AccessPoint => 1001,
            ObjectType::AccessZone => 1002,
            ObjectType::AccessZone2 => 1003,
            ObjectType::AckedSummarization => 1004,
            ObjectType::Action => 1005,
            ObjectType::AlarmSummary => 1006,
            ObjectType::AlarmSummary2 => 1007,
            ObjectType::Annotation => 1008,
            ObjectType::Attachment => 1009,
            ObjectType::AuthParam => 1010,
            ObjectType::AuthPolicy => 1011,
            ObjectType::Cred => 1012,
            ObjectType::CredentialDataInput => 1013,
            ObjectType::Door => 1014,
            ObjectType::NetworkSecurity => 1015,
            ObjectType::NetworkPort => 1016,
            ObjectType::ProtocolObject => 1017,
            ObjectType::ProtocolServices => 1018,
            ObjectType::ProtocolTransaction => 1019,
            ObjectType::StructuredRel => 1020,
            ObjectType::SecurityKey => 1021,
            ObjectType::SecurityKeyset => 1022,
            ObjectType::SecurityLevel => 1023,
            ObjectType::Service => 1024,
            ObjectType::TimePatternValue => 1025,
            ObjectType::TimeSeries => 1026,
            ObjectType::Unknown(v) => *v,
        }
    }
}

impl fmt::Display for ObjectType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ObjectType::AnalogInput => write!(f, "analog-input"),
            ObjectType::AnalogOutput => write!(f, "analog-output"),
            ObjectType::AnalogValue => write!(f, "analog-value"),
            ObjectType::BinaryInput => write!(f, "binary-input"),
            ObjectType::BinaryOutput => write!(f, "binary-output"),
            ObjectType::BinaryValue => write!(f, "binary-value"),
            ObjectType::Calendar => write!(f, "calendar"),
            ObjectType::Command => write!(f, "command"),
            ObjectType::Device => write!(f, "device"),
            ObjectType::EventEnrollment => write!(f, "event-enrollment"),
            ObjectType::File => write!(f, "file"),
            ObjectType::Group => write!(f, "group"),
            ObjectType::Loop => write!(f, "loop"),
            ObjectType::MultiStateInput => write!(f, "multi-state-input"),
            ObjectType::MultiStateOutput => write!(f, "multi-state-output"),
            ObjectType::MultiStateValue => write!(f, "multi-state-value"),
            ObjectType::NotificationClass => write!(f, "notification-class"),
            ObjectType::Program => write!(f, "program"),
            ObjectType::Schedule => write!(f, "schedule"),
            ObjectType::Averaging => write!(f, "averaging"),
            ObjectType::CharacterStringValue => write!(f, "character-string-value"),
            ObjectType::DateTimeValue => write!(f, "date-time-value"),
            ObjectType::IntegerValue => write!(f, "integer-value"),
            ObjectType::LargeAnalogValue => write!(f, "large-analog-value"),
            ObjectType::MultiStateValue2 => write!(f, "multi-state-value2"),
            ObjectType::PositiveIntegerValue => write!(f, "positive-integer-value"),
            ObjectType::TimeValue => write!(f, "time-value"),
            ObjectType::AccessPoint => write!(f, "access-point"),
            ObjectType::AccessZone => write!(f, "access-zone"),
            ObjectType::AccessZone2 => write!(f, "access-zone2"),
            ObjectType::AckedSummarization => write!(f, "acked-summarization"),
            ObjectType::Action => write!(f, "action"),
            ObjectType::AlarmSummary => write!(f, "alarm-summary"),
            ObjectType::AlarmSummary2 => write!(f, "alarm-summary2"),
            ObjectType::Annotation => write!(f, "annotation"),
            ObjectType::Attachment => write!(f, "attachment"),
            ObjectType::AuthParam => write!(f, "auth-param"),
            ObjectType::AuthPolicy => write!(f, "auth-policy"),
            ObjectType::Cred => write!(f, "cred"),
            ObjectType::CredentialDataInput => write!(f, "credential-data-input"),
            ObjectType::Door => write!(f, "door"),
            ObjectType::NetworkSecurity => write!(f, "network-security"),
            ObjectType::NetworkPort => write!(f, "network-port"),
            ObjectType::ProtocolObject => write!(f, "protocol-object"),
            ObjectType::ProtocolServices => write!(f, "protocol-services"),
            ObjectType::ProtocolTransaction => write!(f, "protocol-transaction"),
            ObjectType::StructuredRel => write!(f, "structured-rel"),
            ObjectType::SecurityKey => write!(f, "security-key"),
            ObjectType::SecurityKeyset => write!(f, "security-keyset"),
            ObjectType::SecurityLevel => write!(f, "security-level"),
            ObjectType::Service => write!(f, "service"),
            ObjectType::TimePatternValue => write!(f, "time-pattern-value"),
            ObjectType::TimeSeries => write!(f, "time-series"),
            ObjectType::Unknown(v) => write!(f, "unknown({})", v),
        }
    }
}

pub trait Encode {
    fn encode(&self, buf: &mut Vec<u8>);
}

pub trait Decode: Sized {
    fn decode(data: &[u8]) -> Result<(Self, usize), DecodeError>;
}

#[derive(Debug, Clone, PartialEq)]
pub enum DecodeError {
    IncompleteData,
    InvalidData,
    InvalidTagNumber,
    InvalidContextTag,
    UnsupportedApplicationTag,
    InvalidDate,
    InvalidTime,
    InvalidObjectType,
    UnknownPropertyType,
    UnsupportedBvlcFunction,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecodeError::IncompleteData => write!(f, "incomplete data"),
            DecodeError::InvalidData => write!(f, "invalid data"),
            DecodeError::InvalidTagNumber => write!(f, "invalid tag number"),
            DecodeError::InvalidContextTag => write!(f, "invalid context tag"),
            DecodeError::UnsupportedApplicationTag => write!(f, "unsupported application tag"),
            DecodeError::InvalidDate => write!(f, "invalid date"),
            DecodeError::InvalidTime => write!(f, "invalid time"),
            DecodeError::InvalidObjectType => write!(f, "invalid object type"),
            DecodeError::UnknownPropertyType => write!(f, "unknown property type"),
            DecodeError::UnsupportedBvlcFunction => write!(f, "unsupported BVLC function code"),
        }
    }
}

impl std::error::Error for DecodeError {}

const IS_CONTEXT_TAG: u8 = 0x08;

impl PropertyValue {
    pub fn encode_tag_and_value(&self, buf: &mut Vec<u8>) {
        match self {
            PropertyValue::Null => {
                buf.push(0x00);
            }
            PropertyValue::Boolean(b) => {
                buf.push(0x11);
                buf.push(if *b { 1 } else { 0 });
            }
            PropertyValue::Unsigned(u) => {
                encode_unsigned_app(*u, buf);
            }
            PropertyValue::Signed(s) => {
                encode_signed_app(*s, buf);
            }
            PropertyValue::Real(r) => {
                buf.push(0x44);
                buf.extend_from_slice(&r.to_be_bytes());
            }
            PropertyValue::Double(d) => {
                buf.push(0x55);
                buf.extend_from_slice(&d.to_be_bytes());
            }
            PropertyValue::OctetString(bytes) => {
                // BACnet OctetString: app tag 6, length is byte count
                let value_len = bytes.len() as u32;
                let inline_len = std::cmp::min(value_len, 5) as u8;
                buf.push(0x60 | inline_len);
                buf.extend_from_slice(bytes);
            }
            PropertyValue::CharacterString(cs) => {
                // BACnet CharacterString: app tag 7, length includes encoding byte
                // Limit to 5 chars for inline encoding in v1
                let value_len = cs.value.len() as u32 + 1; // +1 for encoding byte
                let inline_len = std::cmp::min(value_len, 5) as u8;
                buf.push(0x70 | inline_len);
                buf.push(cs.encoding.to_u8());
                buf.extend_from_slice(cs.value.as_bytes());
            }
            PropertyValue::BitString(bs) => {
                // BACnet BitString: app tag 8, length includes used-bits byte
                let value_len = bs.bits.len() as u32 + 1; // +1 for used_bits byte
                let inline_len = std::cmp::min(value_len, 5) as u8;
                buf.push(0x80 | inline_len);
                buf.push(bs.used_bits);
                buf.extend_from_slice(&bs.bits);
            }
            PropertyValue::Enumerated(e) => {
                encode_enumerated(*e, buf);
            }
            PropertyValue::Date(d) => {
                buf.push(0xA0); // tag=10 (Date)
                let year = d.year.saturating_sub(1900);
                buf.push((year / 100) as u8); // century
                buf.push((year % 100) as u8); // year in century
                buf.push(d.month);
                buf.push(d.day);
                if let Some(w) = d.weekday.as_ref() {
                    buf.push(w.to_u8());
                }
            }
            PropertyValue::Time(t) => {
                buf.push(0xB0); // tag=11, no context bit
                buf.push(t.hour);
                buf.push(t.minute);
                buf.push(t.second);
                buf.push(t.hundredths);
            }
            PropertyValue::ObjectIdentifier(oi) => {
                // BACnet ObjectIdentifier: application tag 12
                buf.push(0xC4); // tag=12, length=4 inline
                let value = oi.to_u32();
                buf.push((value >> 24) as u8);
                buf.push((value >> 16) as u8);
                buf.push((value >> 8) as u8);
                buf.push(value as u8);
            }
            PropertyValue::Any(data) => {
                buf.extend_from_slice(data);
            }
            PropertyValue::Array(items) => {
                for item in items {
                    item.encode_tag_and_value(buf);
                }
            }
        }
    }

    pub fn decode(data: &[u8]) -> Result<(PropertyValue, usize), DecodeError> {
        if data.is_empty() {
            return Err(DecodeError::IncompleteData);
        }

        let tag = data[0];
        let tag_number = (tag >> 4) & 0x0F;
        let is_context = (tag & IS_CONTEXT_TAG) != 0;

        if is_context {
            return Err(DecodeError::InvalidContextTag);
        }

        match tag_number {
            0 => decode_null(data),
            1 => decode_boolean(data),
            2 => decode_unsigned(data),
            3 => decode_signed(data),
            4 => decode_real(data),
            5 => decode_double(data),
            6 => decode_octet_string(data),
            7 => decode_character_string(data),
            8 => decode_bit_string(data),
            9 => decode_enumerated(data),
            10 => decode_date(data),
            11 => decode_time(data),
            12 => decode_object_identifier(data),
            _ => Err(DecodeError::UnsupportedApplicationTag),
        }
    }
}

fn encode_unsigned_app(value: u32, buf: &mut Vec<u8>) {
    if value <= 0xFF {
        buf.push(0x21); // tag=2 (Unsigned), length=1 inline
        buf.push(value as u8);
    } else if value <= 0xFFFF {
        buf.push(0x22); // tag=2, length=2 inline
        buf.push((value >> 8) as u8);
        buf.push(value as u8);
    } else if value <= 0xFFFFFF {
        buf.push(0x23); // tag=2, length=3 inline
        buf.push((value >> 16) as u8);
        buf.push((value >> 8) as u8);
        buf.push(value as u8);
    } else {
        buf.push(0x24); // tag=2, length=4 inline
        buf.push((value >> 24) as u8);
        buf.push((value >> 16) as u8);
        buf.push((value >> 8) as u8);
        buf.push(value as u8);
    }
}

fn encode_enumerated(value: u32, buf: &mut Vec<u8>) {
    if value <= 0xFF {
        buf.push(0x91); // tag=9 (Enumerated), length=1 inline
        buf.push(value as u8);
    } else if value <= 0xFFFF {
        buf.push(0x92); // tag=9, length=2 inline
        buf.push((value >> 8) as u8);
        buf.push(value as u8);
    } else if value <= 0xFFFFFF {
        buf.push(0x93); // tag=9, length=3 inline
        buf.push((value >> 16) as u8);
        buf.push((value >> 8) as u8);
        buf.push(value as u8);
    } else {
        buf.push(0x94); // tag=9, length=4 inline
        buf.push((value >> 24) as u8);
        buf.push((value >> 16) as u8);
        buf.push((value >> 8) as u8);
        buf.push(value as u8);
    }
}

fn encode_signed_app(value: i32, buf: &mut Vec<u8>) {
    #[allow(clippy::manual_range_contains)]
    if value >= 0 && value <= 0xFF {
        buf.push(0x31); // tag=3 (Signed), length=1 inline
        buf.push(value as u8);
    } else if value >= 0 && value <= 0xFFFF {
        buf.push(0x32); // tag=3, length=2 inline
        buf.push((value >> 8) as u8);
        buf.push(value as u8);
    } else if value >= -0x800000 && value <= 0x7FFFFF {
        buf.push(0x33); // tag=3, length=3 inline
        buf.push((value >> 16) as u8);
        buf.push((value >> 8) as u8);
        buf.push(value as u8);
    } else {
        buf.push(0x34); // tag=3, length=4 inline
        buf.push((value >> 24) as u8);
        buf.push((value >> 16) as u8);
        buf.push((value >> 8) as u8);
        buf.push(value as u8);
    }
}

fn decode_null(_data: &[u8]) -> Result<(PropertyValue, usize), DecodeError> {
    Ok((PropertyValue::Null, 1))
}

fn decode_boolean(data: &[u8]) -> Result<(PropertyValue, usize), DecodeError> {
    if data.len() < 2 {
        return Err(DecodeError::IncompleteData);
    }
    Ok((PropertyValue::Boolean(data[1] != 0), 2))
}

fn decode_unsigned(data: &[u8]) -> Result<(PropertyValue, usize), DecodeError> {
    if data.len() < 2 {
        return Err(DecodeError::IncompleteData);
    }
    let tag = data[0];
    let length_in_tag = tag & 0x07;

    let len = if length_in_tag < 7 {
        // Length is in tag
        length_in_tag as usize
    } else {
        // Extended length follows
        if data.len() < 3 {
            return Err(DecodeError::IncompleteData);
        }
        let extended_len = data[1];
        match extended_len {
            0x21 => 1,
            0x22 => 2,
            0x23 => 3,
            0x24 => 4,
            _ => return Err(DecodeError::InvalidData),
        }
    };

    let offset = if length_in_tag < 7 { 1 } else { 2 };
    if data.len() < offset + len {
        return Err(DecodeError::IncompleteData);
    }

    let mut value = 0u32;
    for i in 0..len {
        value = (value << 8) | data[offset + i] as u32;
    }
    Ok((PropertyValue::Unsigned(value), offset + len))
}

fn decode_signed(data: &[u8]) -> Result<(PropertyValue, usize), DecodeError> {
    if data.len() < 2 {
        return Err(DecodeError::IncompleteData);
    }
    let tag = data[0];
    let length_in_tag = tag & 0x07;

    let len = if length_in_tag < 7 {
        length_in_tag as usize
    } else {
        if data.len() < 3 {
            return Err(DecodeError::IncompleteData);
        }
        let extended_len = data[1];
        match extended_len {
            0x31 => 1,
            0x32 => 2,
            0x33 => 3,
            0x34 => 4,
            _ => return Err(DecodeError::InvalidData),
        }
    };

    let offset = if length_in_tag < 7 { 1 } else { 2 };
    if data.len() < offset + len {
        return Err(DecodeError::IncompleteData);
    }

    let mut value = 0i32;
    for i in 0..len {
        value = (value << 8) | data[offset + i] as i32;
    }
    if len < 4 {
        let sign_bit = 1 << (len * 8 - 1);
        if value & sign_bit != 0 {
            value |= !((1 << (len * 8)) - 1);
        }
    }
    Ok((PropertyValue::Signed(value), offset + len))
}

fn decode_real(data: &[u8]) -> Result<(PropertyValue, usize), DecodeError> {
    if data.len() < 5 {
        return Err(DecodeError::IncompleteData);
    }
    let bytes = [data[1], data[2], data[3], data[4]];
    let value = f32::from_be_bytes(bytes);
    Ok((PropertyValue::Real(value), 5))
}

fn decode_double(data: &[u8]) -> Result<(PropertyValue, usize), DecodeError> {
    if data.len() < 9 {
        return Err(DecodeError::IncompleteData);
    }
    let bytes = [
        data[1], data[2], data[3], data[4], data[5], data[6], data[7], data[8],
    ];
    let value = f64::from_be_bytes(bytes);
    Ok((PropertyValue::Double(value), 9))
}

fn decode_octet_string(data: &[u8]) -> Result<(PropertyValue, usize), DecodeError> {
    if data.len() < 2 {
        return Err(DecodeError::IncompleteData);
    }
    let tag = data[0];
    let length_in_tag = tag & 0x07;

    let len = if length_in_tag < 7 {
        length_in_tag as usize
    } else {
        if data.len() < 3 {
            return Err(DecodeError::IncompleteData);
        }
        match data[1] {
            0x21 => 1,
            0x22 => 2,
            0x23 => 3,
            0x24 => 4,
            _ => return Err(DecodeError::InvalidData),
        }
    };

    let offset = if length_in_tag < 7 { 1 } else { 2 };
    if data.len() < offset + len {
        return Err(DecodeError::IncompleteData);
    }
    let bytes = data[offset..offset + len].to_vec();
    Ok((PropertyValue::OctetString(bytes), offset + len))
}

fn decode_character_string(data: &[u8]) -> Result<(PropertyValue, usize), DecodeError> {
    if data.len() < 2 {
        return Err(DecodeError::IncompleteData);
    }
    let tag = data[0];
    let length_in_tag = tag & 0x07;

    // CharacterString: length includes the encoding byte
    let total_len = if length_in_tag < 7 {
        length_in_tag as usize
    } else {
        // Extended length - read the length byte
        if data.len() < 3 {
            return Err(DecodeError::IncompleteData);
        }
        let len_byte = data[1];
        if !(0x21..=0x24).contains(&len_byte) {
            return Err(DecodeError::InvalidData);
        }
        (len_byte - 0x20) as usize
    };

    let offset = if length_in_tag < 7 { 1 } else { 2 };

    if data.len() < offset + total_len {
        return Err(DecodeError::IncompleteData);
    }

    let value_len = total_len.saturating_sub(1);
    let encoding = CharacterEncoding::from_u8(data[offset]);
    let value = if value_len > 0 {
        String::from_utf8_lossy(&data[offset + 1..offset + total_len]).to_string()
    } else {
        String::new()
    };

    Ok((
        PropertyValue::CharacterString(CharacterString { encoding, value }),
        offset + total_len,
    ))
}

fn decode_bit_string(data: &[u8]) -> Result<(PropertyValue, usize), DecodeError> {
    if data.len() < 2 {
        return Err(DecodeError::IncompleteData);
    }
    let tag = data[0];
    let length_in_tag = tag & 0x07;

    // BitString: length includes the used-bits byte
    let total_len = if length_in_tag < 7 {
        length_in_tag as usize
    } else {
        if data.len() < 3 {
            return Err(DecodeError::IncompleteData);
        }
        match data[1] {
            0x21 => 1,
            0x22 => 2,
            0x23 => 3,
            0x24 => 4,
            _ => return Err(DecodeError::InvalidData),
        }
    };

    let offset = if length_in_tag < 7 { 1 } else { 2 };
    if data.len() < offset + total_len {
        return Err(DecodeError::IncompleteData);
    }

    let used_bits = data[offset];
    let bits = data[offset + 1..offset + total_len].to_vec();
    Ok((
        PropertyValue::BitString(BitString::new(used_bits, bits)),
        offset + total_len,
    ))
}

fn decode_enumerated(data: &[u8]) -> Result<(PropertyValue, usize), DecodeError> {
    let (pv, len) = decode_unsigned(data)?;
    match pv {
        PropertyValue::Unsigned(u) => Ok((PropertyValue::Enumerated(u), len)),
        _ => Err(DecodeError::InvalidData),
    }
}

fn decode_date(data: &[u8]) -> Result<(PropertyValue, usize), DecodeError> {
    if data.len() < 5 {
        return Err(DecodeError::IncompleteData);
    }
    let century = data[1] as u16 * 100;
    let year = century + data[2] as u16 + 1900;
    let month = data[3];
    let day = data[4];

    if month == 0 || month > 12 || day == 0 || day > 31 {
        return Err(DecodeError::InvalidDate);
    }

    let weekday = if data.len() >= 6 {
        Weekday::from_u8(data[5])
    } else {
        None
    };

    let len = if weekday.is_some() { 6 } else { 5 };
    Ok((
        PropertyValue::Date(Date::new(year, month, day, weekday)),
        len,
    ))
}

fn decode_time(data: &[u8]) -> Result<(PropertyValue, usize), DecodeError> {
    if data.len() < 5 {
        return Err(DecodeError::IncompleteData);
    }
    let hour = data[1];
    let minute = data[2];
    let second = data[3];
    let hundredths = data[4];

    if hour > 23 || minute > 59 || second > 59 || hundredths > 99 {
        return Err(DecodeError::InvalidTime);
    }

    Ok((
        PropertyValue::Time(Time::new(hour, minute, second, hundredths)),
        5,
    ))
}

fn decode_object_identifier(data: &[u8]) -> Result<(PropertyValue, usize), DecodeError> {
    if data.len() < 5 {
        return Err(DecodeError::IncompleteData);
    }
    let value = ((data[1] as u32) << 24)
        | ((data[2] as u32) << 16)
        | ((data[3] as u32) << 8)
        | (data[4] as u32);
    Ok((
        PropertyValue::ObjectIdentifier(ObjectIdentifier::from_u32(value)),
        5,
    ))
}

impl ObjectIdentifier {
    pub fn encode(&self, buf: &mut Vec<u8>) {
        let value = self.to_u32();
        buf.push(0x0C);
        buf.push((value >> 24) as u8);
        buf.push((value >> 16) as u8);
        buf.push((value >> 8) as u8);
        buf.push(value as u8);
    }

    pub fn decode(data: &[u8]) -> Result<(Self, usize), DecodeError> {
        decode_object_identifier(data).map(|(pv, len)| match pv {
            PropertyValue::ObjectIdentifier(oi) => (oi, len),
            _ => unreachable!("decode_object_identifier always returns ObjectIdentifier"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip_boolean_true() {
        roundtrip(PropertyValue::Boolean(true));
    }

    #[test]
    fn test_roundtrip_boolean_false() {
        roundtrip(PropertyValue::Boolean(false));
    }

    #[test]
    fn test_roundtrip_unsigned() {
        roundtrip(PropertyValue::Unsigned(42));
        roundtrip(PropertyValue::Unsigned(0));
        roundtrip(PropertyValue::Unsigned(0xFFFFFFFF));
    }

    #[test]
    fn test_roundtrip_signed() {
        roundtrip(PropertyValue::Signed(42));
        roundtrip(PropertyValue::Signed(-1));
        roundtrip(PropertyValue::Signed(-128));
        roundtrip(PropertyValue::Signed(127));
    }

    #[test]
    fn test_roundtrip_real() {
        roundtrip(PropertyValue::Real(3.14));
        roundtrip(PropertyValue::Real(0.0));
        roundtrip(PropertyValue::Real(-1.0));
    }

    #[test]
    fn test_roundtrip_double() {
        roundtrip(PropertyValue::Double(3.14159265358979));
    }

    #[test]
    fn test_roundtrip_octet_string() {
        // Limit to 4 bytes for inline length encoding
        roundtrip(PropertyValue::OctetString(vec![0x01, 0x02, 0x03]));
    }

    #[test]
    fn test_roundtrip_character_string() {
        // "Hell" (4 chars) + 1 encoding byte = 5, fits in inline length
        let original = PropertyValue::CharacterString(CharacterString {
            encoding: CharacterEncoding::Utf8,
            value: "Hell".to_string(),
        });
        let mut encoded = Vec::new();
        original.encode_tag_and_value(&mut encoded);
        let (decoded, consumed) = PropertyValue::decode(&encoded).unwrap();
        assert_eq!(consumed, encoded.len());
        assert_eq!(decoded, original);
    }

    #[test]
    fn test_roundtrip_bit_string() {
        roundtrip(PropertyValue::BitString(BitString::new(8, vec![0xAA])));
    }

    #[test]
    fn test_roundtrip_enumerated() {
        roundtrip(PropertyValue::Enumerated(0));
        roundtrip(PropertyValue::Enumerated(255));
    }

    #[test]
    fn test_roundtrip_date() {
        roundtrip(PropertyValue::Date(Date::new(
            2024,
            4,
            16,
            Some(Weekday::Tuesday),
        )));
    }

    #[test]
    fn test_roundtrip_time() {
        roundtrip(PropertyValue::Time(Time::new(14, 30, 0, 0)));
    }

    #[test]
    fn test_roundtrip_object_identifier() {
        roundtrip(PropertyValue::ObjectIdentifier(ObjectIdentifier::new(
            ObjectType::Device,
            1,
        )));
        roundtrip(PropertyValue::ObjectIdentifier(ObjectIdentifier::new(
            ObjectType::AnalogInput,
            42,
        )));
    }

    fn roundtrip(original: PropertyValue) {
        let mut encoded = Vec::new();
        original.encode_tag_and_value(&mut encoded);
        let (decoded, consumed) = PropertyValue::decode(&encoded).unwrap();
        assert_eq!(consumed, encoded.len());
        assert_eq!(decoded, original);
    }
}
