"""Check literal workflow action pins and forbid unlocked Cargo build fallbacks.

Also forbids piping a download into a shell and installing a floating pip
requirement. This checks repository workflow text, not transitive actions,
downloaded tools, runner images or package registries. It deliberately rejects
nonliteral uses.
"""

from pathlib import Path
import re
import sys
import unittest

USES = re.compile(r"^\s*(?:-\s*)?uses:\s*(.*?)\s*$")
PIN = re.compile(r"[\w.-]+/[\w./-]+@[0-9a-f]{40}\Z")
PIPED_INSTALLER = re.compile(r"\b(?:curl|wget)\b[^\n|]*\|\s*(?:sudo\s+)?(?:ba|z|da)?sh\b")
PIP_INSTALL = re.compile(r"\bpip3?\s+install\b([^\n]*)")
PIP_FLAGS_WITH_VALUE = {"-r", "--requirement", "-c", "--constraint", "-t", "--target"}


def floating_requirements(arguments):
    """Requirements on a pip install line that do not name an exact version.

    A shell variable or a path is an artifact this workflow built, so it floats
    on nothing.
    """
    floating = []
    tokens = arguments.split("#", 1)[0].split()
    index = 0
    while index < len(tokens):
        token = tokens[index].strip("'\"")
        index += 1
        if token in PIP_FLAGS_WITH_VALUE:
            index += 1
        elif token.startswith("-") or "$" in token or "/" in token or "==" in token:
            continue
        elif token.endswith(".whl") or token == ".":
            continue
        else:
            floating.append(token)
    return floating


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
    for number, line in enumerate(text.splitlines(), 1):
        if line.lstrip().startswith("#"):
            continue
        if PIPED_INSTALLER.search(line):
            errors.append(f"line {number}: piped installer; pin the tool instead")
    joined = text.replace("\\\n", " ")
    for line in joined.splitlines():
        if re.match(r"\s*(?:#|(?:-\s*)?name:)", line):
            continue
        for match in PIP_INSTALL.finditer(line):
            errors.extend(
                f"pip requirement must name an exact version: {requirement}"
                for requirement in floating_requirements(match.group(1))
            )
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

    def test_piped_installer_fails_and_a_piped_archive_passes(self):
        self.assertTrue(violations("run: curl https://x.test/init.sh -sSf | sh"))
        self.assertTrue(violations("run: wget -qO- https://x.test/i | sudo bash"))
        self.assertEqual(violations("curl -sSL https://x.test/a.tgz | tar -xz"), [])
        self.assertEqual(violations("# curl https://x.test/i | sh"), [])

    def test_floating_pip_requirements_fail(self):
        for line in (
            "pip install --upgrade pip maturin",
            "pip install --upgrade pip",
            'python -m pip install "numpy==2.5.3" pytest',
            "pip install numpy>=2",
            "pip install --upgrade \\\n  maturin",
        ):
            with self.subTest(line=line):
                self.assertTrue(violations("run: " + line))

    def test_exact_files_and_built_artifacts_pass(self):
        for line in (
            'pip install "pip==26.2.2" "maturin==1.15.0"',
            "pip install --requirement build-requirements.txt",
            'python -m pip install --quiet "pytest==9.1.1"',
            './consumer/bin/pip install --quiet "$WHEEL"',
            "pip install dist/sharpebench-1.0.0-py3-none-any.whl",
            "pip install sharpebench.whl",
            "# `pip install <extra>` resolves the extra",
            "    name: maturin build, pip install the wheel, import",
        ):
            with self.subTest(line=line):
                self.assertEqual(violations(line), [])


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
        print(
            "CI policy: literal external action commits; no Cargo build fallback; "
            "no piped installer; exact pip requirements"
        )
