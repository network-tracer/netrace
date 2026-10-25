/// Small, seedable, deterministic random number generator (SplitMix64).
///
/// Used for anything in the simulation that needs randomness (MAC address
/// generation, protocol jitter, DHCP transaction IDs) so runs stay replayable.
/// Not suitable for cryptography.
#[derive(Debug, Clone)]
pub struct Rng {
    state: u64,
}

impl Rng {
    pub fn seeded(seed: u64) -> Self {
        Rng { state: seed }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    pub fn next_u32(&mut self) -> u32 {
        (self.next_u64() >> 32) as u32
    }

    /// Uniform value in `low..high`. Panics if the range is empty.
    pub fn range(&mut self, low: u64, high: u64) -> u64 {
        assert!(low < high, "empty range {low}..{high}");
        let span = high - low;
        // Rejection sampling avoids modulo bias.
        let zone = u64::MAX - (u64::MAX % span);
        loop {
            let v = self.next_u64();
            if v < zone {
                return low + v % span;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_sequence() {
        let mut a = Rng::seeded(42);
        let mut b = Rng::seeded(42);
        for _ in 0..100 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn different_seeds_diverge() {
        let mut a = Rng::seeded(1);
        let mut b = Rng::seeded(2);
        assert_ne!(a.next_u64(), b.next_u64());
    }

    #[test]
    fn known_first_output() {
        // Reference value for SplitMix64 seeded with 0.
        assert_eq!(Rng::seeded(0).next_u64(), 0xE220_A839_7B1D_CDAF);
    }

    #[test]
    fn range_stays_in_bounds() {
        let mut r = Rng::seeded(7);
        for _ in 0..1_000 {
            let v = r.range(10, 20);
            assert!((10..20).contains(&v));
        }
    }
}
