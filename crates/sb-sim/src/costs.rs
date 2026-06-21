//! Transaction costs + a tiny seeded PRNG for execution noise.
//!
//! Realistic costs (fees, slippage, and seed-varying execution noise) are what
//! make pass^k meaningful: the same agent run under different execution seeds
//! produces slightly different returns, so a one-seed fluke can't top the board.

/// Basis-point transaction cost model.
#[derive(Clone, Copy, Debug)]
pub struct CostModel {
    pub fee_bps: f64,
    pub slippage_bps: f64,
}

impl Default for CostModel {
    fn default() -> Self {
        Self {
            fee_bps: 2.0,
            slippage_bps: 3.0,
        }
    }
}

/// Minimal deterministic PRNG (SplitMix64) for seeded execution noise.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed ^ 0xA5A5_5A5A_C3C3_3C3C)
    }
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    /// Uniform in [-1, 1].
    pub fn signed_unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0
    }
}
