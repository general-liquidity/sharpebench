// Historical 2026-09-07 baseline reproduction; see README.md for the exact commit.
// Assertions characterize pre-repair behavior and are expected to fail after repair.
use sharpebench_sim::data::Dataset;

fn main() {
    let original = Dataset::synthetic(1, 3, 1).with_dividend_yield(0.01);
    let masked = original.masked();
    let original_dividend = original.dividend_at("SYM00", 0);
    let masked_dividend = masked.dividend_at("ASSET_000", 0);
    assert!(original_dividend > 0.0);
    assert_eq!(masked_dividend, 0.0);
    println!("R19: dividend original={original_dividend} masked={masked_dividend}");

    let nonfinite = Dataset::from_csv(
        "date,symbol,close\n2025-01-01,A,NaN\n2025-01-02,A,100\n",
    )
    .unwrap();
    assert!(nonfinite.close_at("A", 0).unwrap().is_nan());
    let duplicate = Dataset::from_csv(
        "date,symbol,close,dividend\n2025-01-01,A,100,1\n2025-01-01,A,101,garbage\n2025-01-02,A,102,2\n",
    )
    .unwrap();
    assert_eq!(duplicate.close_at("A", 0), Some(101.0));
    assert_eq!(duplicate.dividend_at("A", 0), 1.0);
    println!("R20: nonfinite accepted; duplicate close=101, dividend=1");
}
