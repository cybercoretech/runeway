//! A small seeded PRNG (xorshift64*), so every lane's level is the same
//! each time for the same agent.

/// Deterministic random numbers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rng(pub u64);

impl Rng {
    /// Seeded from an id (FNV-1a), never zero.
    pub fn for_id(id: &str) -> Rng {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in id.bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
        Rng(hash.max(1))
    }

    /// A new generator for one part of the world (a level chunk).
    pub fn fork(&self, salt: i64) -> Rng {
        let mut rng = Rng((self.0 ^ (salt as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)).max(1));
        rng.next_u64();
        rng
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    /// A number in `0..n` (0 when `n` is 0).
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 { 0 } else { self.next_u64() % n }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_id_same_numbers() {
        let (mut a, mut b) = (Rng::for_id("x"), Rng::for_id("x"));
        assert_eq!(a.next_u64(), b.next_u64());
        assert_ne!(Rng::for_id("x").next_u64(), Rng::for_id("y").next_u64());
    }

    #[test]
    fn below_stays_in_range() {
        let mut rng = Rng::for_id("r");
        assert!((0..1000).all(|_| rng.below(7) < 7));
        assert_eq!(rng.below(0), 0);
    }
}
