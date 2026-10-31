"""Tests for scripts/check-lean-scope.py: each check must fail for its own planted cause.

Run with ``python3 -m unittest discover -s scripts -p 'test_check_lean_scope.py'``.
"""

from __future__ import annotations

import importlib.util
import pathlib
import tempfile
import textwrap
import unittest

SCRIPT = pathlib.Path(__file__).resolve().with_name("check-lean-scope.py")
SPEC = importlib.util.spec_from_file_location("check_lean_scope", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
scope = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(scope)
LIBRARY = scope.LIBRARY

ROOT_MODULE = f"""\
/-!
## Scope

Covers: no rule of its own; it re-exports `formal/{LIBRARY}/Model.lean`.

Assumes: nothing.
-/
"""

MODEL = """\
/-!
## Main results

- `step_le`: one step stays under the cap.

## Scope

Covers: `adjust` in `src/prod.rs`, modelled as `step`.

Assumes: natural numbers.
-/

def step (a cap : Nat) : Nat := Nat.min cap a

theorem step_le (a cap : Nat) : step a cap ≤ cap := Nat.min_le_left _ _
"""

NON_VACUITY = f"""\
/-!
Witnesses for `step_le`.

## Scope

Covers: the model in `formal/{LIBRARY}/Model.lean`.

Assumes: nothing.
-/
"""


class ScopeCheck(unittest.TestCase):
    def setUp(self) -> None:
        self._dir = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self._dir.name)
        self.write(f"formal/{LIBRARY}.lean", ROOT_MODULE)
        self.write(f"formal/{LIBRARY}/Model.lean", MODEL)
        self.write(f"formal/{LIBRARY}/NonVacuity.lean", NON_VACUITY)
        self.write("src/prod.rs", "pub fn adjust(x: f64) -> f64 { x.min(1.0) }\n")

    def tearDown(self) -> None:
        self._dir.cleanup()

    def write(self, rel: str, text: str) -> None:
        path = self.root / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(textwrap.dedent(text), encoding="utf-8")

    def replace(self, rel: str, old: str, new: str) -> None:
        path = self.root / rel
        text = path.read_text(encoding="utf-8")
        self.assertIn(old, text)
        path.write_text(text.replace(old, new), encoding="utf-8")

    def errors(self) -> list[str]:
        return scope.run(self.root)

    def assert_single_error(self, fragment: str) -> None:
        errors = self.errors()
        self.assertEqual(len(errors), 1, errors)
        self.assertIn(fragment, errors[0])

    def test_well_formed_library_passes(self) -> None:
        self.assertEqual(self.errors(), [])

    def test_renamed_production_symbol_fails(self) -> None:
        self.write("src/prod.rs", "pub fn adjust_capped(x: f64) -> f64 { x.min(1.0) }\n")
        self.assert_single_error("production symbol 'adjust'")

    def test_lean_name_in_scope_block_is_not_a_production_symbol(self) -> None:
        # `step` occurs in no production file, but it is a name the model's code declares.
        self.assertNotIn("step", (self.root / "src/prod.rs").read_text(encoding="utf-8"))
        self.assertEqual(self.errors(), [])

    def test_symbol_named_only_in_a_comment_of_the_model_is_still_checked(self) -> None:
        self.replace(f"formal/{LIBRARY}/Model.lean", "`adjust`", "`clamp`")
        self.assert_single_error("production symbol 'clamp'")

    def test_missing_path_fails(self) -> None:
        self.replace(f"formal/{LIBRARY}/Model.lean", "`src/prod.rs`", "`src/gone.rs`")
        errors = self.errors()
        self.assertTrue(any("does not exist: src/gone.rs" in error for error in errors), errors)

    def test_missing_scope_block_fails(self) -> None:
        self.write(f"formal/{LIBRARY}/Extra.lean", "def extra : Nat := 0\n")
        self.assert_single_error("missing a '## Scope' block")

    def test_missing_assumes_line_fails(self) -> None:
        self.replace(f"formal/{LIBRARY}/Model.lean", "Assumes: natural numbers.", "")
        self.assert_single_error("no 'Assumes:' line")

    def test_main_result_that_is_not_declared_fails(self) -> None:
        self.replace(f"formal/{LIBRARY}/Model.lean", "theorem step_le", "theorem step_le'")
        errors = self.errors()
        self.assertTrue(any("lists 'step_le'" in error for error in errors), errors)

    def test_main_result_that_is_only_a_def_fails(self) -> None:
        self.replace(f"formal/{LIBRARY}/Model.lean", "- `step_le`", "- `step`")
        self.assert_single_error("lists 'step'")

    def test_theorem_without_non_vacuity_witness_fails(self) -> None:
        self.replace(f"formal/{LIBRARY}/NonVacuity.lean", "`step_le`", "the step")
        self.assert_single_error("theorem 'step_le' has no witness")

    def test_missing_non_vacuity_module_fails(self) -> None:
        (self.root / f"formal/{LIBRARY}/NonVacuity.lean").unlink()
        self.assert_single_error("missing; every theorem needs a witness")

    def test_theorem_inside_a_comment_is_not_counted(self) -> None:
        self.replace(f"formal/{LIBRARY}/Model.lean", "theorem step_le", "-- theorem step_le\ntheorem x")
        errors = self.errors()
        self.assertTrue(any("lists 'step_le'" in error for error in errors), errors)

    def test_repository_passes(self) -> None:
        self.assertEqual(scope.run(scope.ROOT), [])


if __name__ == "__main__":
    unittest.main()
