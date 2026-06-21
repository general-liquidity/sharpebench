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
    /// Own-order market-impact coefficient (bps at 100% participation). Slippage
    /// grows with the square root of the trade's share of portfolio NAV, so an
    /// agent that wins by betting huge pays for the size it moves.
    pub impact_bps: f64,
}

impl Default for CostModel {
    fn default() -> Self {
        Self {
            fee_bps: 2.0,
            slippage_bps: 3.0,
            impact_bps: 50.0,
        }
    }
}

/// Own-order market impact as a return fraction: a concave (square-root law)
/// function of `participation` = |trade value| / portfolio NAV. Concavity is the
/// empirical Almgren shape — the first dollar moves the price more than the last.
pub fn market_impact_frac(impact_bps: f64, participation: f64) -> f64 {
    impact_bps / 10_000.0 * participation.max(0.0).sqrt()
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

    /// Uniform in [0, 1).
    pub fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn impact_grows_with_participation() {
        let small = market_impact_frac(50.0, 0.01);
        let big = market_impact_frac(50.0, 0.5);
        assert!(big > small, "bigger trade should cost more");
        assert!(market_impact_frac(50.0, 0.0).abs() < 1e-12);
    }

    #[test]
    fn impact_is_concave() {
        // Square-root law: doubling participation less-than-doubles the impact.
        let a = market_impact_frac(50.0, 0.1);
        let b = market_impact_frac(50.0, 0.2);
        assert!(b < 2.0 * a, "impact must be concave in size");
    }
}
