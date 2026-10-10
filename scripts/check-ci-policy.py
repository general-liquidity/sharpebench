"""Check literal workflow action pins and forbid unlocked Cargo build fallbacks.

This checks repository workflow text, not transitive actions, downloaded tools,
runner images or package registries. It deliberately rejects nonliteral uses.
"""

from pathlib import Path
import re
import sys
import unittest

USES = re.compile(r"^\s*(?:-\s*)?uses:\s*(.*?)\s*$")
PIN = re.compile(r"[\w.-]+/[\w./-]+@[0-9a-f]{40}\Z")


def violations(text):
    errors = []
    for number, line in enumerate(text.splitlines(), 1):
        match = USES.match(line)
        if match:
            reference = match.group(1).split(" #", 1)[0].strip().strip("'\"")
            if not reference.startswith("./") and not PIN.fullmatch(reference):
                errors.append(
                    f"line {number}: action requires a full commit SHA: {reference}"
                )
    joined = text.replace("\\\n", " ")
    if re.search(r"cargo\s+build\b[^\n]*\|\|", joined):
        errors.append(
            "Cargo build must fail rather than fall back to an unlocked build"
        )
    return errors


class PolicyTests(unittest.TestCase):
    def test_pinned_action_and_local_action(self):
        self.assertEqual(violations("- uses: org/action@" + "a" * 40 + " # v2"), [])
        self.assertEqual(violations("    uses: ./local-action"), [])

    def test_tags_branches_expressions_and_short_shas_fail(self):
        for reference in (
            "org/action@v2",
            "org/action@main",
            "org/action@abc123",
            "org/action@${{ inputs.ref }}",
            ">",
        ):
            with self.subTest(reference=reference):
                self.assertTrue(violations("  - uses: " + reference))

    def test_quoted_pins_and_reusable_workflows(self):
        self.assertEqual(
            violations(
                "uses: 'org/repo/.github/workflows/job.yml@" + "b" * 40 + "' # reviewed"
            ),
            [],
        )

    def test_comment_is_not_an_action(self):
        self.assertEqual(violations("# - uses: org/action@main"), [])

    def test_locked_build_and_original_fallback(self):
        self.assertEqual(violations("run: cargo build --workspace --locked"), [])
        self.assertTrue(
            violations(
                "run: cargo build --workspace --locked || cargo build --workspace"
            )
        )

    def test_split_line_fallback(self):
        self.assertTrue(violations("cargo build --locked \\\n || cargo build"))


def check(root):
    directory = root / ".github" / "workflows"
    files = sorted([*directory.glob("*.yml"), *directory.glob("*.yaml")])
    if not files:
        return ["no workflow files found"]
    return [
        f"{file.relative_to(root)}: {error}"
        for file in files
        for error in violations(file.read_text(encoding="utf-8"))
    ]


if __name__ == "__main__":
    if sys.argv[1:] == ["--self-test"]:
        unittest.main(argv=[sys.argv[0]])
    elif sys.argv[1:]:
        raise SystemExit("usage: check-ci-policy.py [--self-test]")
    else:
        errors = check(Path(__file__).resolve().parents[1])
        if errors:
            print("\n".join(errors))
            raise SystemExit(1)
        print("CI policy: literal external action commits; no Cargo build fallback")
