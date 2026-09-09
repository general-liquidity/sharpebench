"""Scanner tests for the paired-boundary gate, on a synthetic crate layout."""

from __future__ import annotations

import importlib.util
from pathlib import Path
import sys
import tempfile
import textwrap
import unittest


_SPEC = importlib.util.spec_from_file_location(
    "check_paired_boundaries", Path(__file__).with_name("check-paired-boundaries.py")
)
assert _SPEC is not None and _SPEC.loader is not None
gate = importlib.util.module_from_spec(_SPEC)
# Dataclasses resolve their module through sys.modules when annotations are strings.
sys.modules[_SPEC.name] = gate
_SPEC.loader.exec_module(gate)


SOURCE = textwrap.dedent(
    '''
    //! Synthetic kernel module.

    /// Inputs must be finite and `alpha` in (0, 1).
    pub fn domained(values: &[f64], alpha: f64) -> f64 {
        values.len() as f64 * alpha
    }

    /// Confidence level in [0, 1]; the paired test lives in the boundary file.
    pub fn covered_by_file(confidence: f64) -> f64 {
        confidence
    }

    /// Documents nothing about the domain of `x`.
    pub fn undocumented(x: f64) -> f64 {
        x
    }

    /// Finite text only: no numeric input, so not a candidate.
    pub fn textual(label: &str) -> usize {
        label.len()
    }

    pub(crate) fn not_public(alpha: f64) -> f64 {
        alpha
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// A brace inside a format string: "{" must not unbalance the scan.
        #[test]
        fn domained_boundary_alpha_edges() {
            assert!(domained(&[1.0], 0.0).is_finite(), "{}", "}");
            let _ = "{{ not a brace }}";
        }

        /// Not a boundary test; it names covered_by_file but does not count.
        #[test]
        fn plain_test() {
            let _ = covered_by_file(0.5);
        }

        /// Inside the test module: this `pub fn` is not a candidate.
        /// alpha must be finite.
        pub fn helper(alpha: f64) -> f64 {
            alpha
        }
    }
    '''
)

BOUNDARY_FILE = textwrap.dedent(
    """
    use kernel::covered_by_file;

    fn probe(c: f64) -> f64 {
        covered_by_file(c)
    }

    #[test]
    fn confidence_edges_are_handled() {
        assert_eq!(probe(0.0), 0.0);
        assert_eq!(probe(1.0), 1.0);
    }
    """
)


class PairedBoundaryScannerTests(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.root = Path(self._tmp.name)
        crate = self.root / "crates" / "sharpebench-kernel"
        (crate / "src").mkdir(parents=True)
        (crate / "tests").mkdir()
        (crate / "src" / "module.rs").write_text(SOURCE, encoding="utf-8")
        (crate / "tests" / "kernel_boundaries.rs").write_text(BOUNDARY_FILE, encoding="utf-8")
        self.crates = ("crates/sharpebench-kernel",)

    def tearDown(self) -> None:
        self._tmp.cleanup()

    def test_candidates_need_a_numeric_input_and_a_documented_domain(self) -> None:
        report = gate.scan(self.root, self.crates)
        self.assertEqual(
            [c.key for c in report.candidates],
            ["kernel::module::domained", "kernel::module::covered_by_file"],
        )
        domained = report.candidates[0]
        self.assertEqual(domained.domains, ("finite", "in (0, 1)", "alpha"))
        self.assertEqual(domained.line, 5)

    def test_a_boundary_named_test_or_a_boundary_file_covers(self) -> None:
        report = gate.scan(self.root, self.crates)
        self.assertEqual({c.name for c in report.covered}, {"domained", "covered_by_file"})
        self.assertEqual(report.uncovered, [])

    def test_a_plain_test_does_not_cover(self) -> None:
        crate = self.root / "crates" / "sharpebench-kernel"
        (crate / "tests" / "kernel_boundaries.rs").unlink()
        report = gate.scan(self.root, self.crates)
        self.assertEqual([c.name for c in report.uncovered], ["covered_by_file"])

    def test_the_allowlist_is_a_ratchet(self) -> None:
        crate = self.root / "crates" / "sharpebench-kernel"
        (crate / "tests" / "kernel_boundaries.rs").unlink()
        report = gate.scan(self.root, self.crates)

        failures, notes = gate.evaluate(report, frozenset())
        self.assertEqual(len(failures), 1)
        self.assertIn("`covered_by_file`", failures[0])
        self.assertEqual(notes, ["paired-boundary gate: 2 candidates, 1 covered, 1 uncovered (0 allowlisted)"])

        failures, _ = gate.evaluate(report, frozenset({"kernel::module::covered_by_file"}))
        self.assertEqual(failures, [])

        failures, _ = gate.evaluate(
            report,
            frozenset({"kernel::module::covered_by_file", "kernel::module::domained"}),
        )
        self.assertEqual(len(failures), 1)
        self.assertIn("is now covered", failures[0])

        failures, _ = gate.evaluate(
            report, frozenset({"kernel::module::covered_by_file", "kernel::module::gone"})
        )
        self.assertEqual(len(failures), 1)
        self.assertIn("names no current candidate", failures[0])

    def test_brace_matching_skips_literals_and_comments(self) -> None:
        text = 'fn f() { let s = "}"; let c = \'}\'; let r = r#"}"#; // }\n /* } */ let l: &\'a str = ""; }'
        self.assertEqual(gate.matching_brace(text, text.index("{")), len(text) - 1)

    def test_the_repository_allowlist_matches_the_tree(self) -> None:
        report = gate.scan(gate.ROOT)
        failures, _ = gate.evaluate(report, gate.ALLOWLIST)
        self.assertEqual(failures, [])


if __name__ == "__main__":
    unittest.main()
