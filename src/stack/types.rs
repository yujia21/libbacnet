use crate::codec::types::ObjectIdentifier;
use crate::enums::PropertyIdentifier;
use crate::stack::addr::BacnetAddr;

// ─────────────────────────────────────────────────────────────────────────────
// Input
// ─────────────────────────────────────────────────────────────────────────────

/// An input event driven into the stack by the host.
#[derive(Debug, Clone)]
pub enum Input {
    /// A UDP datagram was received.
    Received { data: Vec<u8>, src: BacnetAddr },
    /// The host's timer fired. `now` is seconds since an arbitrary epoch.
    Tick { now: f64 },
    /// The application wants to send a confirmed service request.
    Send {
        service: BacnetService,
        dest: BacnetAddr,
    },
}

// ─────────────────────────────────────────────────────────────────────────────
// Output
// ─────────────────────────────────────────────────────────────────────────────

/// An output produced by the stack for the host to act on.
#[derive(Debug, Clone)]
pub enum Output {
    /// Send these bytes to `dest` via UDP.
    Transmit { data: Vec<u8>, dest: BacnetAddr },
    /// An application-level event has occurred.
    Event(BacnetEvent),
    /// The host must call `Tick` no later than this timestamp (same epoch as `Tick.now`).
    Deadline(f64),
}

// ─────────────────────────────────────────────────────────────────────────────
// BacnetEvent + UnconfirmedMessage
// ─────────────────────────────────────────────────────────────────────────────

/// Application-level events emitted by the stack.
#[derive(Debug, Clone)]
pub enum BacnetEvent {
    /// A confirmed response was received and the payload is fully reassembled.
    Response { invoke_id: u8, payload: Vec<u8> },
    /// No response was received within the retry budget.
    Timeout { invoke_id: u8 },
    /// The server sent an Abort PDU.
    Abort { invoke_id: u8, reason: u8 },
    /// The server sent an Error PDU, or a local error occurred.
    Error { invoke_id: u8, message: String },
    /// An unconfirmed message was received (I-Am, IAmRouterToNetwork, …).
    UnconfirmedReceived {
        src: BacnetAddr,
        message: UnconfirmedMessage,
    },
}

/// Payload of an `UnconfirmedReceived` event.
#[derive(Debug, Clone)]
pub enum UnconfirmedMessage {
    IAm {
        device_id: ObjectIdentifier,
        max_apdu: u32,
        /// Segmentation-supported encoded as raw u8 (0=Both, 1=Tx, 2=Rx, 3=None).
        segmentation: u8,
        vendor_id: u32,
    },
    IAmRouterToNetwork {
        networks: Vec<u16>,
    },
}

// ─────────────────────────────────────────────────────────────────────────────
// BacnetService — the service variants the stack can send
// ─────────────────────────────────────────────────────────────────────────────

/// A confirmed BACnet service request to be sent.
#[derive(Debug, Clone)]
pub enum BacnetService {
    ReadProperty {
        object_id: ObjectIdentifier,
        property_id: PropertyIdentifier,
        array_index: Option<u32>,
    },
    ReadPropertyMultiple {
        specs: Vec<crate::services::read_property_multiple::ReadAccessSpec>,
    },
    WriteProperty {
        object_id: ObjectIdentifier,
        property_id: PropertyIdentifier,
        value: crate::codec::types::PropertyValue,
        array_index: Option<u32>,
        priority: Option<u8>,
    },
}

impl BacnetService {
    /// Returns the BACnet service choice byte for this service variant.
    ///
    /// Used as the service-choice octet in the APDU header.
    pub fn choice(&self) -> u8 {
        match self {
            BacnetService::ReadProperty { .. } => crate::services::read_property::SERVICE_CHOICE,
            BacnetService::ReadPropertyMultiple { .. } => {
                crate::services::read_property_multiple::SERVICE_CHOICE
            }
            BacnetService::WriteProperty { .. } => crate::services::write_property::SERVICE_CHOICE,
        }
    }

    /// Encode the service-request data bytes (no APDU header).
    ///
    /// Returns the raw bytes that follow the APDU header in a
    /// `ConfirmedRequest` PDU.  The stack wraps these bytes with the
    /// APDU, NPDU, and BVLC headers before transmission.
    pub fn encode(&self) -> Vec<u8> {
        match self {
            BacnetService::ReadProperty {
                object_id,
                property_id,
                array_index,
            } => crate::services::read_property::encode_request(
                object_id,
                *property_id,
                *array_index,
            ),
            BacnetService::ReadPropertyMultiple { specs } => {
                crate::services::read_property_multiple::encode_request(specs)
            }
            BacnetService::WriteProperty {
                object_id,
                property_id,
                value,
                array_index,
                priority,
            } => crate::services::write_property::encode_request(
                object_id,
                *property_id,
                value,
                *array_index,
                *priority,
            ),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// StackConfig
// ─────────────────────────────────────────────────────────────────────────────

/// Configuration parameters for the `Stack`.
#[derive(Debug, Clone)]
pub struct StackConfig {
    /// Timeout before retransmitting a confirmed request (seconds).
    pub apdu_timeout_secs: f64,
    /// Maximum number of retransmissions before giving up.
    pub apdu_retries: u8,
    /// Maximum APDU length for outgoing segmentation.
    pub max_apdu_length: usize,
    /// Maximum segment buffer size for reassembly (bytes).
    pub max_segment_buffer: usize,
}

impl Default for StackConfig {
    fn default() -> Self {
        Self {
            apdu_timeout_secs: 3.0,
            apdu_retries: 3,
            max_apdu_length: 1476,
            max_segment_buffer: 2 * 1024 * 1024, // 2 MB
        }
    }
}
