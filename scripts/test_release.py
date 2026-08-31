"""Pure and temporary-repository tests for the isolated release driver."""

from __future__ import annotations

import importlib.util
import tempfile
import unittest
from pathlib import Path


_SPEC = importlib.util.spec_from_file_location(
    "sharpebench_release", Path(__file__).with_name("release.py")
)
assert _SPEC is not None and _SPEC.loader is not None
release = importlib.util.module_from_spec(_SPEC)
_SPEC.loader.exec_module(release)


class ReleaseDriverTests(unittest.TestCase):
    def test_versions_are_computed_without_touching_the_tree(self) -> None:
        self.assertEqual(release.next_version("0.15.0", "patch"), "0.15.1")
        self.assertEqual(release.next_version("0.15.0", "minor"), "0.16.0")
        self.assertEqual(release.next_version("0.15.0", "major"), "1.0.0")
        self.assertEqual(release.next_version("0.15.0", "2.3.4"), "2.3.4")

    def test_a_subheading_without_entries_is_empty(self) -> None:
        self.assertEqual(
            release.unreleased_entries(
                "# Changelog\n\n## [Unreleased]\n\n### Changed\n\n## [0.1.0]\n"
            ),
            [],
        )
        self.assertEqual(
            release.unreleased_entries(
                "# Changelog\n\n## [Unreleased]\n\n### Changed\n- real entry\n\n## [0.1.0]\n"
            ),
            ["- real entry"],
        )

    def test_the_pushed_base_comparison_keeps_release_categories(self) -> None:
        added = "## [Unreleased]\n\n### Added\n- the same words\n\n## [0.1.0]\n"
        changed = added.replace("### Added", "### Changed")
        self.assertEqual(release.unreleased_entries(added), release.unreleased_entries(changed))
        self.assertNotEqual(release.unreleased_section(added), release.unreleased_section(changed))

    def test_cleanup_preserves_registration_when_removal_fails(self) -> None:
        present = True
        calls: list[str] = []

        def remove() -> None:
            calls.append("remove")
            raise OSError("busy")

        steps = release.cleanup_worktree(
            is_present=lambda: present,
            remove=remove,
            deregister=lambda: calls.append("deregister"),
        )
        self.assertEqual(steps, ("remove-failed",))
        self.assertEqual(calls, ["remove"])

    def test_changelog_promotion_moves_entries_and_commits_them(self) -> None:
        with tempfile.TemporaryDirectory(prefix="sharpebench-release-test-") as raw:
            root = Path(raw)
            (root / "CHANGELOG.md").write_text(
                "# Changelog\n\n"
                "[Unreleased]: https://example.test/v0.1.0...HEAD\n\n"
                "## [Unreleased]\n\n### Added\n- a real change\n\n"
                "## [0.1.0] - 2026-01-01\n\n"
                "[0.1.0]: https://example.test/v0.1.0\n",
                encoding="utf-8",
                newline="\n",
            )
            release.run(root, "git", "init", "--quiet", "--initial-branch=main")
            release.run(root, "git", "add", "CHANGELOG.md")
            release.run(root, "git", "commit", "--quiet", "-m", "initial")

            release.prepare_changelog(root, "0.1.0", "0.2.0")

            text = (root / "CHANGELOG.md").read_text(encoding="utf-8")
            promoted = text.split("## [0.2.0]", 1)[1].split("## [0.1.0]", 1)[0]
            self.assertIn("- a real change", promoted)
            self.assertIn("compare/v0.2.0...HEAD", text)
            self.assertEqual(
                release.git(root, "log", "-1", "--format=%s"),
                "docs(changelog): prepare v0.2.0",
            )


if __name__ == "__main__":
    unittest.main()
