//! Deterministic `exp` and `ln` in pure Rust.
//!
//! `f64::exp` and `f64::ln` lower to LLVM intrinsics. At run time they call the
//! platform libm; when the argument is a compile-time constant, an optimised
//! build folds the call inside the compiler instead, with whatever `exp` the
//! compiler process links. The two need not agree. On `x86_64-pc-windows-gnu`
//! with rustc 1.96.0, `exp(-0.5000000000000001)` is `0x3fe368b2fc6f9609` from
//! the runtime libm (correctly rounded) and `0x3fe368b2fc6f960a` from the
//! release-mode fold (0.61 ulp above the true value), which moved the pinned
//! bits of `erf(FRAC_1_SQRT_2)` between `cargo test` and `cargo test --release`.
//!
//! The functions here use only `+`, `-`, `*`, `/` and bit manipulation, which
//! IEEE 754 defines exactly and which LLVM folds exactly, so a debug build, a
//! release build and every platform return the same bits. Each evaluates the
//! result in double-double arithmetic (about 100 significant bits) and rounds
//! once at the end, so the output is the correctly rounded value except in
//! rare hard-to-round cases (an argument whose true result lies within roughly
//! 2^-100 relative of a rounding midpoint), where it is still deterministic.
//! Subnormal results of `exp` are rounded twice and may sit one subnormal ulp
//! from the correctly rounded value; they are equally deterministic.

/// A double-double: the unevaluated sum `hi + lo` with `|lo| <= ulp(hi) / 2`.
#[derive(Clone, Copy)]
struct Dd {
    hi: f64,
    lo: f64,
}

/// Error-free sum: returns `(s, e)` with `s = fl(a + b)` and `s + e = a + b`.
fn two_sum(a: f64, b: f64) -> (f64, f64) {
    let s = a + b;
    let bb = s - a;
    (s, (a - (s - bb)) + (b - bb))
}

/// Error-free sum when `|a| >= |b|` (or `a == 0`).
fn quick_two_sum(a: f64, b: f64) -> (f64, f64) {
    let s = a + b;
    (s, b - (s - a))
}

/// Dekker split of `a` into two 26-bit halves with `hi + lo = a`.
fn split(a: f64) -> (f64, f64) {
    let c = 134_217_729.0 * a; // 2^27 + 1
    let hi = c - (c - a);
    (hi, a - hi)
}

/// Error-free product without FMA: `p = fl(a * b)` and `p + e = a * b`.
fn two_prod(a: f64, b: f64) -> (f64, f64) {
    let p = a * b;
    let (ah, al) = split(a);
    let (bh, bl) = split(b);
    (p, ((ah * bh - p) + ah * bl + al * bh) + al * bl)
}

impl Dd {
    const fn from(x: f64) -> Self {
        Dd { hi: x, lo: 0.0 }
    }

    fn norm(hi: f64, lo: f64) -> Self {
        let (hi, lo) = quick_two_sum(hi, lo);
        Dd { hi, lo }
    }

    fn add(self, o: Dd) -> Dd {
        let (s, e) = two_sum(self.hi, o.hi);
        let (t, f) = two_sum(self.lo, o.lo);
        let (s, e) = quick_two_sum(s, e + t);
        Dd::norm(s, e + f)
    }

    fn mul(self, o: Dd) -> Dd {
        let (p, e) = two_prod(self.hi, o.hi);
        Dd::norm(p, e + (self.hi * o.lo + self.lo * o.hi))
    }

    fn mul_f(self, b: f64) -> Dd {
        let (p, e) = two_prod(self.hi, b);
        Dd::norm(p, e + self.lo * b)
    }

    fn div(self, o: Dd) -> Dd {
        // Two Newton-style correction steps on the quotient.
        let q1 = self.hi / o.hi;
        let r = self.add(o.mul_f(-q1));
        let q2 = r.hi / o.hi;
        let r = r.add(o.mul_f(-q2));
        let q3 = r.hi / o.hi;
        let (q, e) = quick_two_sum(q1, q2);
        Dd::from(q).add(Dd { hi: e, lo: 0.0 }).add(Dd::from(q3))
    }

    /// The double nearest to `hi + lo` (the pair is normalised, so it is `hi`).
    fn round(self) -> f64 {
        self.hi + self.lo
    }
}

/// `ln 2` as an unevaluated triple, accurate to about 2^-160.
const LN2_HI: f64 = f64::from_bits(0x3fe6_2e42_fefa_39ef);
const LN2_LO: f64 = f64::from_bits(0x3c7a_bc9e_3b39_803f);
const LN2_LO2: f64 = f64::from_bits(0x3907_b57a_079a_1934);

/// `1/n!` for `n = 2..=12` as double-doubles (from 80-digit decimal arithmetic).
const INV_FACT: [Dd; 11] = [
    Dd {
        hi: f64::from_bits(0x3fe0_0000_0000_0000),
        lo: f64::from_bits(0x0000_0000_0000_0000),
    }, // 1/2!
    Dd {
        hi: f64::from_bits(0x3fc5_5555_5555_5555),
        lo: f64::from_bits(0x3c65_5555_5555_5555),
    }, // 1/3!
    Dd {
        hi: f64::from_bits(0x3fa5_5555_5555_5555),
        lo: f64::from_bits(0x3c45_5555_5555_5555),
    }, // 1/4!
    Dd {
        hi: f64::from_bits(0x3f81_1111_1111_1111),
        lo: f64::from_bits(0x3c01_1111_1111_1111),
    }, // 1/5!
    Dd {
        hi: f64::from_bits(0x3f56_c16c_16c1_6c17),
        lo: f64::from_bits(0xbbef_49f4_9f49_f49f),
    }, // 1/6!
    Dd {
        hi: f64::from_bits(0x3f2a_01a0_1a01_a01a),
        lo: f64::from_bits(0x3b6a_01a0_1a01_a01a),
    }, // 1/7!
    Dd {
        hi: f64::from_bits(0x3efa_01a0_1a01_a01a),
        lo: f64::from_bits(0x3b3a_01a0_1a01_a01a),
    }, // 1/8!
    Dd {
        hi: f64::from_bits(0x3ec7_1de3_a556_c734),
        lo: f64::from_bits(0xbb6c_154f_8ddc_6c00),
    }, // 1/9!
    Dd {
        hi: f64::from_bits(0x3e92_7e4f_b778_9f5c),
        lo: f64::from_bits(0x3b3c_bbc0_5b4f_a99a),
    }, // 1/10!
    Dd {
        hi: f64::from_bits(0x3e5a_e645_67f5_44e4),
        lo: f64::from_bits(0xbafc_062e_06d1_f209),
    }, // 1/11!
    Dd {
        hi: f64::from_bits(0x3e21_eed8_eff8_d898),
        lo: f64::from_bits(0xbac2_aec9_59e1_4c06),
    }, // 1/12!
];

/// `1/(2j+1)` for `j = 0..=24` as double-doubles (from 80-digit decimal arithmetic).
const INV_ODD: [Dd; 25] = [
    Dd {
        hi: f64::from_bits(0x3ff0_0000_0000_0000),
        lo: f64::from_bits(0x0000_0000_0000_0000),
    }, // 1/1
    Dd {
        hi: f64::from_bits(0x3fd5_5555_5555_5555),
        lo: f64::from_bits(0x3c75_5555_5555_5555),
    }, // 1/3
    Dd {
        hi: f64::from_bits(0x3fc9_9999_9999_999a),
        lo: f64::from_bits(0xbc69_9999_9999_999a),
    }, // 1/5
    Dd {
        hi: f64::from_bits(0x3fc2_4924_9249_2492),
        lo: f64::from_bits(0x3c62_4924_9249_2492),
    }, // 1/7
    Dd {
        hi: f64::from_bits(0x3fbc_71c7_1c71_c71c),
        lo: f64::from_bits(0x3c5c_71c7_1c71_c71c),
    }, // 1/9
    Dd {
        hi: f64::from_bits(0x3fb7_45d1_745d_1746),
        lo: f64::from_bits(0xbc47_45d1_745d_1746),
    }, // 1/11
    Dd {
        hi: f64::from_bits(0x3fb3_b13b_13b1_3b14),
        lo: f64::from_bits(0xbc53_b13b_13b1_3b14),
    }, // 1/13
    Dd {
        hi: f64::from_bits(0x3fb1_1111_1111_1111),
        lo: f64::from_bits(0x3c31_1111_1111_1111),
    }, // 1/15
    Dd {
        hi: f64::from_bits(0x3fae_1e1e_1e1e_1e1e),
        lo: f64::from_bits(0x3c2e_1e1e_1e1e_1e1e),
    }, // 1/17
    Dd {
        hi: f64::from_bits(0x3faa_f286_bca1_af28),
        lo: f64::from_bits(0x3c4a_f286_bca1_af28),
    }, // 1/19
    Dd {
        hi: f64::from_bits(0x3fa8_6186_1861_8618),
        lo: f64::from_bits(0x3c48_6186_1861_8618),
    }, // 1/21
    Dd {
        hi: f64::from_bits(0x3fa6_42c8_590b_2164),
        lo: f64::from_bits(0x3c36_42c8_590b_2164),
    }, // 1/23
    Dd {
        hi: f64::from_bits(0x3fa4_7ae1_47ae_147b),
        lo: f64::from_bits(0xbc2e_b851_eb85_1eb8),
    }, // 1/25
    Dd {
        hi: f64::from_bits(0x3fa2_f684_bda1_2f68),
        lo: f64::from_bits(0x3c42_f684_bda1_2f68),
    }, // 1/27
    Dd {
        hi: f64::from_bits(0x3fa1_a7b9_611a_7b96),
        lo: f64::from_bits(0x3c21_a7b9_611a_7b96),
    }, // 1/29
    Dd {
        hi: f64::from_bits(0x3fa0_8421_0842_1084),
        lo: f64::from_bits(0x3c30_8421_0842_1084),
    }, // 1/31
    Dd {
        hi: f64::from_bits(0x3f9f_07c1_f07c_1f08),
        lo: f64::from_bits(0xbc2f_07c1_f07c_1f08),
    }, // 1/33
    Dd {
        hi: f64::from_bits(0x3f9d_41d4_1d41_d41d),
        lo: f64::from_bits(0x3c30_7507_5075_0750),
    }, // 1/35
    Dd {
        hi: f64::from_bits(0x3f9b_acf9_14c1_bad0),
        lo: f64::from_bits(0xbc3b_acf9_14c1_bad0),
    }, // 1/37
    Dd {
        hi: f64::from_bits(0x3f9a_41a4_1a41_a41a),
        lo: f64::from_bits(0x3c30_6906_9069_0690),
    }, // 1/39
    Dd {
        hi: f64::from_bits(0x3f98_f9c1_8f9c_18fa),
        lo: f64::from_bits(0xbc2f_3831_f383_1f38),
    }, // 1/41
    Dd {
        hi: f64::from_bits(0x3f97_d05f_417d_05f4),
        lo: f64::from_bits(0x3c17_d05f_417d_05f4),
    }, // 1/43
    Dd {
        hi: f64::from_bits(0x3f96_c16c_16c1_6c17),
        lo: f64::from_bits(0xbc2f_49f4_9f49_f49f),
    }, // 1/45
    Dd {
        hi: f64::from_bits(0x3f95_c988_2b93_1057),
        lo: f64::from_bits(0x3c23_1057_2620_ae4c),
    }, // 1/47
    Dd {
        hi: f64::from_bits(0x3f94_e5e0_a72f_0539),
        lo: f64::from_bits(0x3c3e_0a72_f053_9783),
    }, // 1/49
];

/// `2^k` for `-1022 <= k <= 1023`.
fn pow2(k: i32) -> f64 {
    debug_assert!((-1022..=1023).contains(&k));
    f64::from_bits(((k + 1023) as u64) << 52)
}

/// Deterministic `e^x`. NaN propagates; `+inf -> +inf`, `-inf -> 0`.
pub fn exp(x: f64) -> f64 {
    if x.is_nan() {
        return x;
    }
    if x > 709.8 {
        return f64::INFINITY;
    }
    if x < -746.0 {
        return 0.0;
    }
    // k = nearest integer to x / ln 2, |k| <= 1077. `round` is exact IEEE.
    let k = (x * std::f64::consts::LOG2_E).round();
    // r = x - k ln 2 in double-double. k * LN2_HI and k * LN2_LO are formed
    // exactly by `two_prod`, so cancellation against x loses nothing.
    let (p1, e1) = two_prod(k, LN2_HI);
    let (p2, e2) = two_prod(k, LN2_LO);
    let r = Dd::from(x)
        .add(Dd { hi: -p1, lo: -e1 })
        .add(Dd { hi: -p2, lo: -e2 })
        .add(Dd::from(-k * LN2_LO2));
    // |r| <= ln2 / 2 + tiny. Scale by 2^-8 (exact) so the series converges fast.
    const SQUARINGS: i32 = 8;
    let s = Dd {
        hi: r.hi * (1.0 / 256.0),
        lo: r.lo * (1.0 / 256.0),
    };
    // u = expm1(s) = s + s^2 (1/2! + s/3! + ... + s^10/12!), |s| <= 1.36e-3,
    // so the first omitted term s^13/13! is below 2^-150.
    // inv: after folding INV_FACT[i], acc = sum_{n=i+2}^{12} s^(n-i-2) / n! (Horner).
    // term: i runs 9 down to 0, a fixed bound.
    let mut acc = INV_FACT[10];
    for c in INV_FACT[..10].iter().rev() {
        acc = acc.mul(s).add(*c);
    }
    let mut u = s.add(s.mul(s).mul(acc));
    // e^(2s) - 1 = 2u + u^2 keeps u's relative accuracy (no cancellation at 1).
    // inv: after i steps, u = expm1(s * 2^i). term: i runs to SQUARINGS.
    for _ in 0..SQUARINGS {
        u = u.mul_f(2.0).add(u.mul(u));
    }
    let y = Dd::from(1.0).add(u).round();
    // Scale by 2^k. k is in [-1077, 1024]; split it so each factor is a normal
    // power of two. Products with normal results are exact; only an overflow
    // or a subnormal result rounds here.
    let k = k as i32;
    if k > 1023 {
        y * pow2(1023) * pow2(k - 1023)
    } else if k < -1022 {
        // y * 2^(k+1022) is normal (k + 1022 >= -54), so the only rounding
        // is the final step into the subnormal range.
        y * pow2(k + 1022) * pow2(-1022)
    } else {
        y * pow2(k)
    }
}

/// Deterministic natural logarithm. NaN and negative arguments give NaN,
/// `+-0 -> -inf`, `+inf -> +inf`.
pub fn ln(x: f64) -> f64 {
    if x.is_nan() || x < 0.0 {
        return f64::NAN;
    }
    if x == 0.0 {
        return f64::NEG_INFINITY;
    }
    if x == f64::INFINITY {
        return x;
    }
    // x = m * 2^e with m in [sqrt(1/2), sqrt(2)).
    let mut bits = x.to_bits();
    let mut e: i64 = 0;
    if bits >> 52 == 0 {
        // Subnormal: scale by 2^54 (exact) into the normal range.
        bits = (x * f64::from_bits(0x4350_0000_0000_0000)).to_bits();
        e -= 54;
    }
    e += ((bits >> 52) & 0x7ff) as i64 - 1023;
    let mut m = f64::from_bits((bits & 0x000f_ffff_ffff_ffff) | 0x3ff0_0000_0000_0000);
    if m > std::f64::consts::SQRT_2 {
        m *= 0.5;
        e += 1;
    }
    // ln m = 2 atanh(s), s = (m - 1) / (m + 1), |s| <= 0.1716.
    // m - 1 is exact (Sterbenz), m + 1 is captured exactly by two_sum.
    let (a_hi, a_lo) = two_sum(m, 1.0);
    let s = Dd::from(m - 1.0).div(Dd { hi: a_hi, lo: a_lo });
    let s2 = s.mul(s);
    // atanh(s)/s = sum_{j>=0} s2^j / (2j+1). s2 <= 0.0295, s2^25 < 2^-125.
    // inv: after folding INV_ODD[j], acc = sum_{i=j}^{24} s2^(i-j) / (2i+1) (Horner).
    // term: j runs 23 down to 0, a fixed bound.
    let mut acc = INV_ODD[24];
    for c in INV_ODD[..24].iter().rev() {
        acc = acc.mul(s2).add(*c);
    }
    let ln_m = s.mul(acc).mul_f(2.0);
    // e ln 2, with e * LN2_HI and e * LN2_LO formed exactly.
    let ef = e as f64;
    let (p1, e1) = two_prod(ef, LN2_HI);
    let (p2, e2) = two_prod(ef, LN2_LO);
    Dd { hi: p1, lo: e1 }
        .add(Dd { hi: p2, lo: e2 })
        .add(Dd::from(ef * LN2_LO2))
        .add(ln_m)
        .round()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exp_matches_the_correctly_rounded_value_the_release_fold_missed() {
        // Exact e^-0.5000000000000001 = 0.60653065971263335626...; the nearest
        // double is 0x3fe368b2fc6f9609 (0.39 ulp away), the LLVM fold gave ...960a.
        let x = f64::from_bits(0xbfe0_0000_0000_0001);
        assert_eq!(exp(x).to_bits(), 0x3fe3_68b2_fc6f_9609);
        assert_eq!(
            exp(std::hint::black_box(x)).to_bits(),
            0x3fe3_68b2_fc6f_9609
        );
    }

    // (argument bits, correctly rounded result bits), the results from 80-digit
    // decimal arithmetic. The first eight of each are arguments where the
    // x86_64-pc-windows-gnu libm returns the other neighbour (0.50-0.54 ulp off).
    const EXP_REF: &[(u64, u64)] = &[
        (0xc010_3e41_be44_46b0, 0x3f91_a61f_1948_01b8),
        (0xc075_dab6_46b6_a49a, 0x2067_28e2_5d29_a6d7),
        (0xc075_a9dd_4077_f067, 0x20ae_a7c9_e346_0353),
        (0xc085_d210_5cd2_86a0, 0x00f8_b42c_67c3_b13b),
        (0xbf60_1418_d168_c694, 0x3fef_efef_f094_344b),
        (0xc061_5fe4_34fb_c3d0, 0x3366_2a86_6fbe_fc16),
        (0xc028_0507_6b64_b3fe, 0x3ed9_84d0_967b_0942),
        (0xc078_30ad_93e4_f472, 0x1d08_8518_91fc_41fd),
        (0xbfe0_0000_0000_0000, 0x3fe3_68b2_fc6f_960a),
        (0x81a5_6e1f_c2f8_f359, 0x3ff0_0000_0000_0000),
        (0xbddb_7cdf_d9d7_bdbb, 0x3fef_ffff_fff2_4190),
        (0xc000_0000_0000_0000, 0x3fc1_52aa_a3bf_81cc),
        (0xc01d_0000_0000_0000, 0x3f47_455f_e323_fafd),
        (0xc049_0000_0000_0000, 0x3b6d_257d_547e_083f),
        (0xc085_e000_0000_0000, 0x00d1_4f2b_0fb9_307f),
        (0x3fb9_9999_9999_999a, 0x3ff1_aec7_b35a_00d4),
        (0x3ff0_0000_0000_0000, 0x4005_bf0a_8b14_5769),
        (0x400c_0000_0000_0000, 0x4040_8ec7_2139_6bdb),
        (0x4034_0000_0000_0000, 0x41bc_eb08_8b68_e804),
        (0x4085_e000_0000_0000, 0x7f0d_945d_f4f8_ec8e),
        (0xbfbb_7903_4d12_0982, 0x3fec_be66_f25f_db35),
        (0xc007_6c53_0900_4c9c, 0x3fab_659e_d67e_62ac),
    ];
    const LN_REF: &[(u64, u64)] = &[
        (0x3fe7_1bff_856a_bb07, 0xbfd4_d4f4_e405_8689),
        (0x3dda_9429_de03_ad35, 0xc037_0f3a_1a1c_603b),
        (0x3fee_b000_1cc3_11d1, 0xbfa5_715a_6c24_04ed),
        (0x3fe2_440c_9647_61ea, 0xbfe1_f14a_9584_05ad),
        (0x3fee_70de_81ec_9c8a, 0xbfa9_92e9_0a20_53d1),
        (0x3fee_027f_d6b3_1d3e, 0xbfb0_7045_a41b_1453),
        (0x1af9_998a_6881_48f9, 0xc079_9df8_5dc9_7b37),
        (0x2658_f393_7a7f_92c9, 0xc071_bbef_9dbf_8ed9),
        (0x3fe0_0000_0000_0000, 0xbfe6_2e42_fefa_39ef),
        (0x3f98_d4fd_f3b6_45a2, 0xc00d_c134_a0d2_8448),
        (0x01a5_6e1f_c2f8_f359, 0xc085_9634_47f8_7fb5),
        (0x3ddb_7cdf_d9d7_bdbb, 0xc037_069e_2aa2_aa5b),
        (0x3fef_3333_3333_3333, 0xbf99_ece9_5532_1b70),
        (0x3fe8_0000_0000_0000, 0xbfd2_6962_1134_db92),
        (0x3ff8_0000_0000_0000, 0x3fd9_f323_ecbf_984c),
        (0x3ff0_0000_0000_0001, 0x3caf_ffff_ffff_ffff),
        (0x3fef_ffff_ffff_ffff, 0xbca0_0000_0000_0000),
        (0x4008_0000_0000_0000, 0x3ff1_93ea_7aad_030b),
        (0x7e37_e43c_8800_759c, 0x4085_9634_47f8_7fb5),
        (0x0010_0000_0000_0000, 0xc086_232b_dd7a_bcd2),
    ];

    #[test]
    fn exp_and_ln_are_correctly_rounded_on_reference_arguments() {
        for &(x, want) in EXP_REF {
            let got = exp(f64::from_bits(x)).to_bits();
            assert_eq!(got, want, "exp({:e})", f64::from_bits(x));
        }
        for &(x, want) in LN_REF {
            let got = ln(f64::from_bits(x)).to_bits();
            assert_eq!(got, want, "ln({:e})", f64::from_bits(x));
        }
    }

    #[test]
    fn exp_and_ln_edge_cases() {
        assert_eq!(exp(0.0), 1.0);
        assert_eq!(exp(-0.0), 1.0);
        assert_eq!(exp(1.0), std::f64::consts::E);
        assert_eq!(exp(f64::INFINITY), f64::INFINITY);
        assert_eq!(exp(f64::NEG_INFINITY), 0.0);
        assert_eq!(exp(710.0), f64::INFINITY);
        assert_eq!(exp(-746.0), 0.0);
        assert_eq!(exp(-745.0), f64::from_bits(1)); // 4.94e-324, smallest subnormal
        assert!(exp(f64::NAN).is_nan());
        assert_eq!(ln(1.0), 0.0);
        assert_eq!(ln(2.0), std::f64::consts::LN_2);
        assert_eq!(ln(10.0), std::f64::consts::LN_10);
        assert_eq!(ln(std::f64::consts::E), 1.0);
        assert_eq!(ln(0.0), f64::NEG_INFINITY);
        assert_eq!(ln(f64::INFINITY), f64::INFINITY);
        assert!(ln(-1.0).is_nan());
        assert!(ln(f64::NAN).is_nan());
        // ln of the smallest subnormal: -744.4400719213812.
        assert_eq!(ln(f64::from_bits(1)), -744.4400719213812);
    }
}
