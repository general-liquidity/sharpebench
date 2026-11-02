//! Seeded path generation. Every stream is a pure function of its key, so a
//! shard's bytes do not depend on which worker ran it or in what order.
//!
//! A path has mean zero and unit variance whatever the dependence, the same
//! effect definition as the development calibration: a planted annualized
//! Sharpe `s` is the constant `s / sqrt(periods_per_year)` added to the path, and
//! it means the same thing in every cell. For GARCH and regime paths the unit
//! variance is the unconditional one, which a finite path only approaches.

use crate::campaign::{Dgp, Regime, Tail};

/// SplitMix64, the mixer the simulator's `Rng` and the kernel's bootstrap use.
/// It is restated here rather than borrowed because a shipped generator's
/// internals are not this study's contract.
#[derive(Clone, Debug)]
pub struct Stream {
    state: u64,
    spare: Option<f64>,
}

const GOLDEN: u64 = 0x9E37_79B9_7F4A_7C15;

fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

impl Stream {
    /// The stream named by `key`, folded position by position so that a
    /// permutation of the key is a different stream.
    pub fn keyed(key: &[u64]) -> Self {
        let mut state = 0x5BA7_2026_0000_0001_u64;
        for &part in key {
            state = mix(state.wrapping_add(GOLDEN) ^ part);
        }
        Self { state, spare: None }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(GOLDEN);
        mix(self.state)
    }

    /// Uniform on `[0, 1)`.
    pub fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Uniform below `n`, `n >= 1`.
    pub fn below(&mut self, n: usize) -> usize {
        ((self.unit() * n as f64) as usize).min(n - 1)
    }

    /// Standard normal by the polar method.
    pub fn normal(&mut self) -> f64 {
        if let Some(z) = self.spare.take() {
            return z;
        }
        loop {
            let u = 2.0 * self.unit() - 1.0;
            let v = 2.0 * self.unit() - 1.0;
            let s = u * u + v * v;
            if s > 0.0 && s < 1.0 {
                let scale = (-2.0 * sharpebench_core::det_math::ln(s) / s).sqrt();
                self.spare = Some(v * scale);
                return u * scale;
            }
        }
    }

    fn innovation(&mut self, tail: Tail) -> f64 {
        match tail {
            Tail::Normal => self.normal(),
            Tail::StudentT { nu } => {
                let chi2: f64 = (0..nu)
                    .map(|_| {
                        let z = self.normal();
                        z * z
                    })
                    .sum();
                let nu = f64::from(nu);
                self.normal() / (chi2 / nu).sqrt() * ((nu - 2.0) / nu).sqrt()
            }
        }
    }
}

/// Stationary calm share, mean and volatility of each regime state, scaled so
/// the mixture has mean zero and variance one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RegimeMoments {
    pub calm_share: f64,
    pub calm_mean: f64,
    pub stress_mean: f64,
    pub calm_vol: f64,
    pub stress_vol: f64,
}

impl RegimeMoments {
    pub fn of(regime: Regime) -> Self {
        let calm_share = (1.0 - regime.stay_stress) / (2.0 - regime.stay_calm - regime.stay_stress);
        let stress_share = 1.0 - calm_share;
        let calm_mean = regime.calm_mean;
        let stress_mean = -calm_mean * calm_share / stress_share;
        let vol2 = regime.stress_vol_ratio * regime.stress_vol_ratio;
        let variance = calm_share * (1.0 + calm_mean * calm_mean)
            + stress_share * (vol2 + stress_mean * stress_mean);
        let scale = variance.sqrt();
        Self {
            calm_share,
            calm_mean: calm_mean / scale,
            stress_mean: stress_mean / scale,
            calm_vol: 1.0 / scale,
            stress_vol: regime.stress_vol_ratio / scale,
        }
    }
}

/// One zero-mean, unit-variance path of `len` periods.
pub fn zero_skill_path(dgp: &Dgp, stream: &mut Stream, len: usize) -> Vec<f64> {
    let mut path = Vec::with_capacity(len);
    let scale = (1.0 - dgp.rho * dgp.rho).sqrt();
    let mut variance: f64 = 1.0;
    let mut ar = 0.0;
    let mut calm = None;
    let moments = dgp.regime.map(RegimeMoments::of);
    for t in 0..len {
        let z = stream.innovation(dgp.tail);
        let shock = variance.sqrt() * z;
        if let Some(g) = dgp.garch {
            variance = (1.0 - g.alpha - g.beta) + g.alpha * shock * shock + g.beta * variance;
        }
        ar = if t == 0 {
            shock
        } else {
            dgp.rho * ar + scale * shock
        };
        let value = match (dgp.regime, moments) {
            (Some(regime), Some(m)) => {
                let is_calm = match calm {
                    None => stream.unit() < m.calm_share,
                    Some(true) => stream.unit() < regime.stay_calm,
                    Some(false) => stream.unit() >= regime.stay_stress,
                };
                calm = Some(is_calm);
                if is_calm {
                    m.calm_mean + m.calm_vol * ar
                } else {
                    m.stress_mean + m.stress_vol * ar
                }
            }
            _ => ar,
        };
        path.push(value);
    }
    path
}

/// Blend a common path into each entrant's own at `shared_variance`, keeping
/// each entrant's unit variance. Zero leaves every path as drawn.
pub fn blend_shared(paths: &mut [Vec<f64>], common: &[f64], shared_variance: f64) {
    if shared_variance == 0.0 {
        return;
    }
    let (on, off) = (shared_variance.sqrt(), (1.0 - shared_variance).sqrt());
    for path in paths {
        for (x, m) in path.iter_mut().zip(common) {
            *x = on * m + off * *x;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dgp(rho: f64, tail: Tail) -> Dgp {
        Dgp {
            id: "t".into(),
            rho,
            tail,
            garch: None,
            regime: None,
            shared_variance: 0.0,
        }
    }

    fn moments(x: &[f64]) -> (f64, f64, f64, f64) {
        let n = x.len() as f64;
        let m = x.iter().sum::<f64>() / n;
        let v = x.iter().map(|a| (a - m).powi(2)).sum::<f64>() / n;
        let k = x.iter().map(|a| (a - m).powi(4)).sum::<f64>() / n / (v * v);
        let lag = x.windows(2).map(|w| (w[0] - m) * (w[1] - m)).sum::<f64>() / (n - 1.0) / v;
        (m, v, k, lag)
    }

    #[test]
    fn a_stream_is_a_pure_function_of_its_key() {
        let draw = |key: &[u64]| {
            let mut s = Stream::keyed(key);
            (0..5).map(|_| s.next_u64()).collect::<Vec<_>>()
        };
        assert_eq!(draw(&[1, 2, 3]), draw(&[1, 2, 3]));
        assert_ne!(draw(&[1, 2, 3]), draw(&[1, 3, 2]));
        assert_ne!(draw(&[1, 2, 3]), draw(&[1, 2, 4]));
    }

    #[test]
    fn known_moments_are_recovered_for_each_family() {
        const N: usize = 400_000;
        let mut s = Stream::keyed(&[7]);
        let (m, v, k, lag) = moments(&zero_skill_path(&dgp(0.0, Tail::Normal), &mut s, N));
        assert!(m.abs() < 0.01 && (v - 1.0).abs() < 0.01, "{m} {v}");
        assert!((k - 3.0).abs() < 0.05 && lag.abs() < 0.01, "{k} {lag}");

        let (m, v, _, lag) = moments(&zero_skill_path(&dgp(0.6, Tail::Normal), &mut s, N));
        assert!(m.abs() < 0.02 && (v - 1.0).abs() < 0.02, "{m} {v}");
        assert!((lag - 0.6).abs() < 0.01, "{lag}");

        // Student t(5): kurtosis 3 + 6 / (5 - 4) = 9 in population, and the
        // sample kurtosis converges slowly, so only its side of 3 and the unit
        // variance are asserted.
        let (_, v, k, _) = moments(&zero_skill_path(
            &dgp(0.0, Tail::StudentT { nu: 5 }),
            &mut s,
            N,
        ));
        assert!((v - 1.0).abs() < 0.03 && k > 5.0, "{v} {k}");

        let garch = Dgp {
            garch: Some(crate::campaign::Garch {
                alpha: 0.08,
                beta: 0.9,
            }),
            ..dgp(0.0, Tail::Normal)
        };
        let path = zero_skill_path(&garch, &mut s, N);
        let (_, v, k, _) = moments(&path);
        let squares: Vec<f64> = path.iter().map(|x| x * x).collect();
        let (_, _, _, sq_lag) = moments(&squares);
        assert!(
            (v - 1.0).abs() < 0.1 && k > 3.3 && sq_lag > 0.05,
            "{v} {k} {sq_lag}"
        );
    }

    #[test]
    fn a_regime_path_has_zero_mean_and_unit_variance() {
        let regime = Regime {
            stay_calm: 0.995,
            stay_stress: 0.98,
            calm_mean: 0.1,
            stress_vol_ratio: 2.0,
        };
        let m = RegimeMoments::of(regime);
        let stress = 1.0 - m.calm_share;
        assert!((m.calm_share * m.calm_mean + stress * m.stress_mean).abs() < 1e-15);
        let variance = m.calm_share * (m.calm_vol.powi(2) + m.calm_mean.powi(2))
            + stress * (m.stress_vol.powi(2) + m.stress_mean.powi(2));
        assert!((variance - 1.0).abs() < 1e-12);

        let d = Dgp {
            regime: Some(regime),
            ..dgp(0.0, Tail::Normal)
        };
        let mut s = Stream::keyed(&[11]);
        let (mean, v, _, _) = moments(&zero_skill_path(&d, &mut s, 2_000_000));
        assert!(mean.abs() < 0.01 && (v - 1.0).abs() < 0.03, "{mean} {v}");
    }

    #[test]
    fn blending_keeps_unit_variance_and_sets_the_cross_correlation() {
        let d = dgp(0.0, Tail::Normal);
        let mut s = Stream::keyed(&[13]);
        let common = zero_skill_path(&d, &mut s, 200_000);
        let mut paths = vec![
            zero_skill_path(&d, &mut s, 200_000),
            zero_skill_path(&d, &mut s, 200_000),
        ];
        blend_shared(&mut paths, &common, 0.5);
        let (_, v, _, _) = moments(&paths[0]);
        let corr = paths[0]
            .iter()
            .zip(&paths[1])
            .map(|(a, b)| a * b)
            .sum::<f64>()
            / 200_000.0;
        assert!(
            (v - 1.0).abs() < 0.02 && (corr - 0.5).abs() < 0.02,
            "{v} {corr}"
        );
    }
}
