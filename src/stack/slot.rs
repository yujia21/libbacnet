use crate::stack::addr::BacnetAddr;

/// An in-flight confirmed request slot.
#[derive(Debug, Clone)]
pub struct InFlightSlot {
    /// The fully encoded BVLC frame to retransmit.
    pub request_bytes: Vec<u8>,
    /// Destination address.
    pub dest: BacnetAddr,
    /// Timestamp when the request was first sent (seconds).
    pub sent_at: f64,
    /// Number of retransmissions already performed (0 = first send).
    pub retry_count: u8,
    /// Timestamp at which the next retry (or timeout check) is due.
    pub next_retry_at: f64,
}
