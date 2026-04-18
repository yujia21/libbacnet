use std::fmt;
use std::net::Ipv4Addr;

/// A BACnet/IP network address (IPv4 + UDP port).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BacnetAddr {
    pub addr: Ipv4Addr,
    pub port: u16,
}

impl BacnetAddr {
    /// Create a new `BacnetAddr` from a 4-octet IPv4 address and a UDP port.
    ///
    /// # Example
    ///
    /// ```rust
    /// use libbacnet::stack::addr::BacnetAddr;
    ///
    /// let addr = BacnetAddr::new([192, 168, 1, 10], 47808);
    /// assert_eq!(addr.to_string(), "192.168.1.10:47808");
    /// ```
    pub fn new(octets: [u8; 4], port: u16) -> Self {
        Self {
            addr: Ipv4Addr::from(octets),
            port,
        }
    }
}

impl fmt::Display for BacnetAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.addr, self.port)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bacnet_addr_equality() {
        let a = BacnetAddr::new([192, 168, 1, 1], 47808);
        let b = BacnetAddr::new([192, 168, 1, 1], 47808);
        let c = BacnetAddr::new([192, 168, 1, 2], 47808);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn test_bacnet_addr_display() {
        let a = BacnetAddr::new([10, 0, 0, 1], 47808);
        assert_eq!(a.to_string(), "10.0.0.1:47808");
    }

    #[test]
    fn test_bacnet_addr_hash() {
        use std::collections::HashMap;
        let mut map = HashMap::new();
        let addr = BacnetAddr::new([1, 2, 3, 4], 47808);
        map.insert(addr, 42u8);
        assert_eq!(map[&addr], 42);
    }
}
