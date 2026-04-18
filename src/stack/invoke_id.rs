/// Per-destination invoke ID pool (0–255).
///
/// Uses a simple free-list backed by a bitset to track which IDs are in use.
pub struct InvokeIdPool {
    /// Bit i is set if invoke ID i is currently in use.
    in_use: [u64; 4], // 4 × 64 = 256 bits
    /// Next ID to try first (round-robin hint).
    next_hint: u8,
}

impl InvokeIdPool {
    pub fn new() -> Self {
        Self {
            in_use: [0u64; 4],
            next_hint: 0,
        }
    }

    /// Allocate the next free invoke ID, or return `None` if all 256 are in use.
    pub fn allocate(&mut self) -> Option<u8> {
        // Search from hint, wrapping around.
        for offset in 0u16..=255 {
            let id = ((self.next_hint as u16 + offset) & 0xFF) as u8;
            if !self.is_in_use(id) {
                self.set_in_use(id, true);
                self.next_hint = id.wrapping_add(1);
                return Some(id);
            }
        }
        None
    }

    /// Return an invoke ID to the pool.
    pub fn free(&mut self, id: u8) {
        self.set_in_use(id, false);
    }

    fn is_in_use(&self, id: u8) -> bool {
        let word = (id / 64) as usize;
        let bit = id % 64;
        (self.in_use[word] >> bit) & 1 == 1
    }

    fn set_in_use(&mut self, id: u8, value: bool) {
        let word = (id / 64) as usize;
        let bit = id % 64;
        if value {
            self.in_use[word] |= 1u64 << bit;
        } else {
            self.in_use[word] &= !(1u64 << bit);
        }
    }
}

impl Default for InvokeIdPool {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_allocate_sequential() {
        let mut pool = InvokeIdPool::new();
        assert_eq!(pool.allocate(), Some(0));
        assert_eq!(pool.allocate(), Some(1));
        assert_eq!(pool.allocate(), Some(2));
    }

    #[test]
    fn test_free_and_reuse() {
        let mut pool = InvokeIdPool::new();
        // Allocate all 256, then free a few and verify they can be reallocated.
        let ids: Vec<u8> = (0..256).filter_map(|_| pool.allocate()).collect();
        assert_eq!(ids.len(), 256);
        assert_eq!(pool.allocate(), None);

        pool.free(42);
        pool.free(100);
        let a = pool.allocate().unwrap();
        let b = pool.allocate().unwrap();
        let mut got = [a, b];
        got.sort_unstable();
        assert_eq!(got, [42, 100]);
        assert_eq!(pool.allocate(), None);
    }

    #[test]
    fn test_exhaust_returns_none() {
        let mut pool = InvokeIdPool::new();
        for _ in 0..256 {
            assert!(pool.allocate().is_some());
        }
        assert_eq!(pool.allocate(), None);
    }

    #[test]
    fn test_free_allows_new_allocation() {
        let mut pool = InvokeIdPool::new();
        for _ in 0..256 {
            pool.allocate().unwrap();
        }
        pool.free(42);
        assert_eq!(pool.allocate(), Some(42));
        assert_eq!(pool.allocate(), None);
    }
}
