//! Window order and overlap: what `check_window_order` refuses, and the
//! `walk_forward` output it is checked against.
//!
//! `walk_forward` keeps its behaviour. The sibling SharpeArena repository pins
//! its output for `(365, 30, 45, 20)` in
//! `crates/sharpearena/contract/attestation/pre-hash/walk-forward-365d-w30-t45-s20.json`
//! (FNV-1a/64 `0c466fc89fcb5805` in `kernel-goldens.json`); those windows
//! overlap, which is why the check exists.

use sharpebench_sim::trajectory::{check_window_order, IndexedWindow, WindowOrderError};
use sharpebench_sim::{walk_forward, Window};

/// The committed SharpeArena pre-hash bytes for `walk_forward(365, 30, 45, 20)`.
const WALK_FORWARD_365_30_45_20: &str = r#"[{"end":75,"start":30},{"end":95,"start":50},{"end":115,"start":70},{"end":135,"start":90},{"end":155,"start":110},{"end":175,"start":130},{"end":195,"start":150},{"end":215,"start":170},{"end":235,"start":190},{"end":255,"start":210},{"end":275,"start":230},{"end":295,"start":250},{"end":315,"start":270},{"end":335,"start":290},{"end":355,"start":310}]"#;

/// Serialized the way the SharpeArena wasm facade serializes it.
fn walk_forward_json(n_days: usize, warmup: usize, test: usize, step: usize) -> String {
    let windows: Vec<serde_json::Value> = walk_forward(n_days, warmup, test, step)
        .into_iter()
        .map(|w| serde_json::json!({ "start": w.start, "end": w.end }))
        .collect();
    serde_json::to_string(&windows).unwrap()
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

fn w(start: usize, end: usize) -> Window {
    Window { start, end }
}

fn at(index: usize, start: usize, end: usize) -> IndexedWindow {
    IndexedWindow { index, start, end }
}

#[test]
fn walk_forward_output_is_unchanged_and_its_overlap_is_refused() {
    let json = walk_forward_json(365, 30, 45, 20);
    assert_eq!(json, WALK_FORWARD_365_30_45_20);
    assert_eq!(fnv1a64(json.as_bytes()), 0x0c46_6fc8_9fcb_5805);

    assert_eq!(
        check_window_order(&walk_forward(365, 30, 45, 20)),
        Err(WindowOrderError::Overlapping {
            earlier: at(0, 30, 75),
            later: at(1, 50, 95),
            shared_start: 50,
            shared_end: 75,
        })
    );
}

#[test]
fn walk_forward_with_step_at_least_test_is_accepted() {
    // The window rule every shipped producer uses: step == test.
    assert_eq!(check_window_order(&walk_forward(200, 20, 60, 60)), Ok(()));
    assert_eq!(check_window_order(&walk_forward(120, 20, 25, 25)), Ok(()));
    // A gap between windows shares nothing either.
    assert_eq!(check_window_order(&walk_forward(365, 30, 45, 50)), Ok(()));
}

#[test]
fn adjacent_windows_are_accepted() {
    assert_eq!(check_window_order(&[w(20, 60), w(60, 100)]), Ok(()));
    assert_eq!(check_window_order(&[w(20, 60)]), Ok(()));
    assert_eq!(check_window_order(&[]), Ok(()));
}

#[test]
fn a_window_starting_inside_the_previous_one_is_refused() {
    assert_eq!(
        check_window_order(&[w(20, 60), w(40, 80)]),
        Err(WindowOrderError::Overlapping {
            earlier: at(0, 20, 60),
            later: at(1, 40, 80),
            shared_start: 40,
            shared_end: 60,
        })
    );
    // One bar of overlap is still a bar counted twice.
    assert_eq!(
        check_window_order(&[w(20, 60), w(59, 100)]),
        Err(WindowOrderError::Overlapping {
            earlier: at(0, 20, 60),
            later: at(1, 59, 100),
            shared_start: 59,
            shared_end: 60,
        })
    );
}

#[test]
fn a_nested_or_same_start_window_is_an_overlap_not_a_reordering() {
    assert_eq!(
        check_window_order(&[w(20, 80), w(30, 50)]),
        Err(WindowOrderError::Overlapping {
            earlier: at(0, 20, 80),
            later: at(1, 30, 50),
            shared_start: 30,
            shared_end: 50,
        })
    );
    assert_eq!(
        check_window_order(&[w(20, 60), w(20, 80)]),
        Err(WindowOrderError::Overlapping {
            earlier: at(0, 20, 60),
            later: at(1, 20, 80),
            shared_start: 20,
            shared_end: 60,
        })
    );
}

#[test]
fn windows_out_of_time_order_are_refused_even_when_disjoint() {
    assert_eq!(
        check_window_order(&[w(60, 100), w(20, 60)]),
        Err(WindowOrderError::Unordered {
            earlier: at(0, 60, 100),
            later: at(1, 20, 60),
        })
    );
    // The first violation is named, with the list positions of both windows.
    assert_eq!(
        check_window_order(&[w(0, 10), w(10, 20), w(30, 40), w(20, 30)]),
        Err(WindowOrderError::Unordered {
            earlier: at(2, 30, 40),
            later: at(3, 20, 30),
        })
    );
}

#[test]
fn a_window_without_bars_is_checked_for_order_only() {
    // [40, 40) holds no bar, so it overlaps nothing, and it does not hide the
    // window before it from the next one.
    assert_eq!(
        check_window_order(&[w(20, 60), w(40, 40), w(60, 100)]),
        Ok(())
    );
    assert_eq!(
        check_window_order(&[w(20, 60), w(40, 40), w(50, 70)]),
        Err(WindowOrderError::Overlapping {
            earlier: at(0, 20, 60),
            later: at(2, 50, 70),
            shared_start: 50,
            shared_end: 60,
        })
    );
    assert_eq!(
        check_window_order(&[w(20, 60), w(60, 60), w(30, 90)]),
        Err(WindowOrderError::Unordered {
            earlier: at(1, 60, 60),
            later: at(2, 30, 90),
        })
    );
}

#[test]
fn the_refusal_names_both_windows_and_the_shared_bars() {
    let overlap = check_window_order(&[w(20, 60), w(40, 80)]).unwrap_err();
    assert_eq!(
        overlap.to_string(),
        "evaluation windows 0 [20, 60) and 1 [40, 80) overlap on bars [40, 60): scoring both would count those bars twice; windows may be adjacent but must not overlap"
    );
    let unordered = check_window_order(&[w(60, 100), w(20, 60)]).unwrap_err();
    assert_eq!(
        unordered.to_string(),
        "evaluation window 1 [20, 60) starts before window 0 [60, 100): windows must be listed in time order"
    );
}
