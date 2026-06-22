//! pass^k reliability — does the agent clear the bar on **every** run, not on
//! average? Stochastic agents (LLMs) can win once by luck; a benchmark that
//! ranks the lucky single run is measuring noise. For safety-relevant suites use
//! [`PassMode::All`] (after Sierra's τ²-bench pass^k).

/// How many of the `k` runs must pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PassMode {
    /// Every run must pass (the strict, safety-grade default).
    All,
    /// At least one run passes.
    Any,
    /// At least `n` runs pass.
    AtLeast(usize),
}

/// Evaluate pass^k given a per-run pass/fail vector.
pub fn pass_k(passed_per_run: &[bool], mode: PassMode) -> bool {
    if passed_per_run.is_empty() {
        return false;
    }
    let n_pass = passed_per_run.iter().filter(|&&b| b).count();
    match mode {
        PassMode::All => n_pass == passed_per_run.len(),
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
    }
}
