#!/usr/bin/env python3
"""Negative controls for the formal gate: each planted defect must fail the gate for its own cause.

A gate that has never been seen to fail proves nothing. This script copies a Lean package to a
scratch directory, confirms the unmodified copy passes, then plants one defect at a time and
asserts the matching gate refuses it:

- `sorry`: `lake build` fails, because the package makes every warning an error;
- `sorry` with warnings-as-errors switched off for that one declaration: the build passes and the
  statement script refuses the dependency on `sorryAx`;
- an extra `axiom` and a theorem using it: the statement script refuses both;
- an undeclared variable in a statement: `lake build` fails, because `autoImplicit` is off;
- a weakened statement that still proves: the build and the axiom check pass, and the regenerated
  statement manifest differs from the committed one;
- a wrong row in the shared case table (when the package has one): `lake build` fails.

The repository is never modified. Usage, from the repository root:

    python3 formal/scripts/negative_controls.py --package formal --library SharpeBenchFormal \\
        --file SharpeBenchFormal/Forecast.lean --weaken OLD NEW \
        [--cases cases/forecast.json --case-mutation OLD NEW]

Exit 0 when every control fails for its planted cause, 1 otherwise.
"""

from __future__ import annotations

import argparse
import pathlib
import shutil
import subprocess
import sys
import tempfile

STATEMENTS = pathlib.Path(__file__).resolve().with_name("Statements.lean")


def run(cwd: pathlib.Path, *command: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        command, cwd=cwd, capture_output=True, text=True, encoding="utf-8", errors="replace"
    )


def build(cwd: pathlib.Path) -> subprocess.CompletedProcess[str]:
    return run(cwd, "lake", "build")


def statements(cwd: pathlib.Path, library: str) -> subprocess.CompletedProcess[str]:
    return run(cwd, "lake", "env", "lean", "--run", str(STATEMENTS), library)


def output(result: subprocess.CompletedProcess[str]) -> str:
    return result.stdout + result.stderr


class Controls:
    def __init__(self, package: pathlib.Path, library: str, target: str) -> None:
        self.package, self.library, self.target = package, library, package / target
        self.original = self.target.read_text(encoding="utf-8")
        self.failures: list[str] = []

    def expect(self, name: str, ok: bool, detail: str) -> None:
        print(f"{'ok  ' if ok else 'FAIL'} {name}")
        if not ok:
            self.failures.append(f"{name}: {detail[-2000:]}")

    def plant(self, text: str) -> None:
        self.target.write_text(text, encoding="utf-8")

    def restore(self) -> None:
        self.target.write_text(self.original, encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--package", required=True, type=pathlib.Path)
    parser.add_argument("--library", required=True)
    parser.add_argument("--file", required=True, help="module to plant defects in, package-relative")
    parser.add_argument("--weaken", nargs=2, required=True, metavar=("OLD", "NEW"))
    parser.add_argument("--cases", help="shared case table, package-relative")
    parser.add_argument(
        "--case-mutation", nargs=2, metavar=("OLD", "NEW"), help="a wrong row for the case table"
    )
    args = parser.parse_args()

    with tempfile.TemporaryDirectory() as scratch:
        package = pathlib.Path(scratch) / "package"
        shutil.copytree(
            args.package, package, ignore=shutil.ignore_patterns(".lake", "samuelson")
        )
        c = Controls(package, args.library, args.file)
        committed = (package / "statements.txt").read_text(encoding="utf-8")

        result = build(package)
        c.expect("unmodified package builds", result.returncode == 0, output(result))
        result = statements(package, args.library)
        c.expect(
            "unmodified package matches the committed manifest",
            result.returncode == 0 and result.stdout == committed,
            output(result),
        )
        if c.failures:
            return report(c)

        c.plant(c.original + "\ntheorem plantedSorry : 1 = 2 := sorry\n")
        result = build(package)
        c.expect(
            "sorry fails the build",
            result.returncode != 0 and "sorry" in output(result),
            output(result),
        )

        c.plant(
            c.original
            + "\nset_option warningAsError false in\ntheorem plantedSorry : 1 = 2 := sorry\n"
        )
        result = build(package)
        refused = statements(package, args.library)
        c.expect(
            "sorry past the build is refused by the axiom check",
            result.returncode == 0
            and refused.returncode != 0
            and "plantedSorry depends on sorryAx" in output(refused),
            output(result) + output(refused),
        )

        c.plant(
            c.original
            + "\naxiom plantedAxiom : 1 = 2\ntheorem usesPlantedAxiom : 1 = 2 := plantedAxiom\n"
        )
        result = build(package)
        refused = statements(package, args.library)
        c.expect(
            "an extra axiom is refused by the axiom check",
            result.returncode == 0
            and refused.returncode != 0
            and "plantedAxiom is a declared axiom" in output(refused)
            and "usesPlantedAxiom depends on" in output(refused),
            output(result) + output(refused),
        )

        c.plant(c.original + "\ntheorem plantedAutoBound (h : n < 0) : False := by omega\n")
        result = build(package)
        c.expect(
            "an undeclared variable fails the build",
            result.returncode != 0 and "Unknown identifier `n`" in output(result),
            output(result),
        )

        old, new = args.weaken
        c.expect("the weakening anchor is present", old in c.original, old)
        c.plant(c.original.replace(old, new, 1))
        result = build(package)
        weakened = statements(package, args.library)
        c.expect(
            "a weakened statement passes build and axiom check but changes the manifest",
            result.returncode == 0 and weakened.returncode == 0 and weakened.stdout != committed,
            output(result) + output(weakened),
        )
        c.restore()

        if args.cases:
            table = package / args.cases
            original = table.read_text(encoding="utf-8")
            bad_old, bad_new = args.case_mutation
            c.expect("the case mutation anchor is present", bad_old in original, bad_old)
            table.write_text(original.replace(bad_old, bad_new, 1), encoding="utf-8")
            result = build(package)
            c.expect(
                "a wrong row in the shared case table fails the build",
                result.returncode != 0 and "Cases" in output(result),
                output(result),
            )
            table.write_text(original, encoding="utf-8")

        result = build(package)
        c.expect("the restored package builds again", result.returncode == 0, output(result))
        return report(c)


def report(c: Controls) -> int:
    for failure in c.failures:
        print(failure, file=sys.stderr)
    return 1 if c.failures else 0


if __name__ == "__main__":
    sys.exit(main())
