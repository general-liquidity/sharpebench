//! pass^k reliability — does the agent clear the bar on **every** run, not on
//! average? Stochastic agents (LLMs) can win once by luck; a benchmark that
//! ranks the lucky single run is measuring noise. For safety-relevant suites use
//! [`PassMode::All`] (after Sierra's τ²-bench pass^k).

use serde::{Deserialize, Serialize};

/// How many of the `k` runs must pass.
///
/// Serialized in snake case (`"all"`, `"any"`, `{"at_least": 3}`) so the mode
/// can be named from a `ScoreConfig` file: an ablation that changes the
/// reliability verdict must be reproducible from config, not from a patched
/// binary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PassMode {
    /// Every run must pass (the strict, safety-grade default).
    #[default]
    All,
    /// At least one run passes.
    Any,
    /// At least `n` runs pass.
    AtLeast(usize),
    /// Every run must pass, and each run is tested on its **excess** return over
    /// the benchmark agent's run in the same (window, seed) cell rather than on
    /// its raw return (see `ScoreConfig::benchmark_agent_id`). "Reliable" then
    /// means "beats owning the universe in every window", a mandate-relative
    /// verdict, instead of "profitable in every window", the all-weather
    /// absolute-return verdict of [`PassMode::All`]. Aggregation is identical to
    /// `All`; only the series under test changes. Opt-in.
    RelativeToBenchmark,
}

/// Evaluate pass^k given a per-run pass/fail vector.
pub fn pass_k(passed_per_run: &[bool], mode: PassMode) -> bool {
    if passed_per_run.is_empty() {
        return false;
    }
    let n_pass = passed_per_run.iter().filter(|&&b| b).count();
    match mode {
        PassMode::All | PassMode::RelativeToBenchmark => n_pass == passed_per_run.len(),
        PassMode::Any => n_pass > 0,
        PassMode::AtLeast(k) => n_pass >= k,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes() {
        let runs = [true, true, false, true];
        assert!(!pass_k(&runs, PassMode::All));
        assert!(pass_k(&runs, PassMode::Any));
        assert!(pass_k(&runs, PassMode::AtLeast(3)));
        assert!(!pass_k(&runs, PassMode::AtLeast(4)));
        assert!(pass_k(&[true, true, true], PassMode::All));
        assert!(!pass_k(&[], PassMode::Any));
        // Relative aggregation is `All`: the series under test changes upstream,
        // the "every run" requirement does not.
        assert!(!pass_k(&runs, PassMode::RelativeToBenchmark));
        assert!(pass_k(&[true, true, true], PassMode::RelativeToBenchmark));
        assert!(!pass_k(&[], PassMode::RelativeToBenchmark));
    }

    #[test]
    fn serde_round_trips_in_snake_case() {
        for (mode, json) in [
            (PassMode::All, "\"all\""),
            (PassMode::Any, "\"any\""),
            (PassMode::AtLeast(3), "{\"at_least\":3}"),
            (PassMode::RelativeToBenchmark, "\"relative_to_benchmark\""),
        ] {
            assert_eq!(serde_json::to_string(&mode).unwrap(), json);
            assert_eq!(serde_json::from_str::<PassMode>(json).unwrap(), mode);
        }
        assert_eq!(PassMode::default(), PassMode::All);
    }
}
