"""Pure and temporary-repository tests for the isolated release driver."""

from __future__ import annotations

import importlib.util
import json
import re
import shutil
import tempfile
import tomllib
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

    def test_release_surface_guard_catches_a_stale_npm_wrapper(self) -> None:
        current = release.workspace_version((release.ROOT / "Cargo.toml").read_bytes())
        paths = (
            "Cargo.toml",
            "Cargo.lock",
            "npm/package.json",
            "npm/package-lock.json",
            "npm/pkg/package.json",
            "npm/mcp/package.json",
            "crates/sharpebench-py/Cargo.toml",
            "crates/sharpebench-py/Cargo.lock",
            "crates/sharpebench-py/pyproject.toml",
        )
        with tempfile.TemporaryDirectory(prefix="sharpebench-version-test-") as raw:
            root = Path(raw)
            for relative in paths:
                source = release.ROOT / relative
                destination = root / relative
                destination.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(source, destination)

            self.assertEqual(release.surface_version_problems(root, current), [])
            wrapper = root / "npm/pkg/package.json"
            payload = json.loads(wrapper.read_text(encoding="utf-8"))
            payload["version"] = "0.0.0"
            wrapper.write_text(json.dumps(payload) + "\n", encoding="utf-8")

            self.assertIn(
                f"npm/pkg/package.json reports 0.0.0, tag requires {current}",
                release.surface_version_problems(root, current),
            )

    def test_release_tag_requires_a_semantic_version(self) -> None:
        self.assertEqual(release.tag_version("v0.16.0"), "0.16.0")
        with self.assertRaises(release.ReleaseError):
            release.tag_version("release-0.16.0")

    def test_multiline_lock_replacements_preserve_json_and_toml(self) -> None:
        config = tomllib.loads(
            (release.ROOT / "crates/sharpebench-cli/release.toml").read_text(
                encoding="utf-8"
            )
        )
        targets = {
            "../../npm/package-lock.json": release.ROOT / "npm/package-lock.json",
            "../sharpebench-py/Cargo.lock": (
                release.ROOT / "crates/sharpebench-py/Cargo.lock"
            ),
        }
        rewritten: dict[str, str] = {}
        for configured, path in targets.items():
            payload = path.read_text(encoding="utf-8")
            for rule in config["pre-release-replacements"]:
                if rule["file"] != configured:
                    continue
                replacement = rule["replace"].replace("{{version}}", "0.15.1")
                payload, count = re.subn(rule["search"], replacement, payload)
                self.assertEqual(count, rule["exactly"])
            rewritten[configured] = payload

        npm_lock = json.loads(rewritten["../../npm/package-lock.json"])
        self.assertEqual(npm_lock["version"], "0.15.1")
        self.assertEqual(npm_lock["packages"][""]["version"], "0.15.1")
        py_lock = tomllib.loads(rewritten["../sharpebench-py/Cargo.lock"])
        local_versions = {
            package["name"]: package["version"]
            for package in py_lock["package"]
            if package["name"] == "sharpebench"
            or package["name"].startswith("sharpebench-")
        }
        self.assertTrue(local_versions)
        self.assertEqual(set(local_versions.values()), {"0.15.1"})

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


def _workspace_crates() -> dict[str, dict]:
    """Every workspace member's manifest, keyed by package name."""
    workspace = tomllib.loads((release.ROOT / "Cargo.toml").read_text(encoding="utf-8"))
    crates = {}
    for member in workspace["workspace"]["members"]:
        path = release.ROOT / member / "Cargo.toml"
        manifest = tomllib.loads(path.read_text(encoding="utf-8"))
        crates[manifest["package"]["name"]] = manifest
    return crates


def _internal_deps(manifest: dict, names: set[str]) -> set[str]:
    """Workspace crates that must already be on the registry for this one to publish.

    A path-only dev-dependency is stripped at publish time, so it orders nothing. One
    that carries a version is resolved by the registry and does.
    """
    deps = set(manifest.get("dependencies", {})) | set(manifest.get("build-dependencies", {}))
    deps |= {
        name
        for name, spec in manifest.get("dev-dependencies", {}).items()
        if isinstance(spec, str) or "version" in spec
    }
    return deps & names


class PublishInventoryTests(unittest.TestCase):
    def setUp(self) -> None:
        path = release.ROOT / "scripts/publish-order.txt"
        self.order = path.read_text(encoding="utf-8").split()
        self.crates = _workspace_crates()
        self.publishable = {
            name
            for name, manifest in self.crates.items()
            if manifest["package"].get("publish", True) is not False
        }

    def test_the_inventory_is_exactly_the_publishable_workspace(self) -> None:
        self.assertEqual(len(self.order), len(set(self.order)))
        self.assertEqual(set(self.order), self.publishable)

    def test_the_inventory_lists_a_crate_after_its_dependencies(self) -> None:
        names = set(self.crates)
        for index, name in enumerate(self.order):
            earlier = set(self.order[:index])
            with self.subTest(crate=name):
                self.assertLessEqual(_internal_deps(self.crates[name], names), earlier)

    def test_each_publisher_reads_the_inventory_and_keeps_no_copy(self) -> None:
        readers = {
            ".github/workflows/release.yml": 2,
            "scripts/publish.sh": 1,
        }
        for path, reads in readers.items():
            text = (release.ROOT / path).read_text(encoding="utf-8")
            with self.subTest(path=path):
                self.assertEqual(len(re.findall(r"mapfile -t \w+ < .*publish-order\.txt", text)), reads)
                self.assertNotRegex(text, r"\bsharpebench-(?:memory|leaderboard)\b")

    def test_the_package_check_only_names_crates_with_no_internal_dependency(self) -> None:
        text = (release.ROOT / "scripts/publish.sh").read_text(encoding="utf-8")
        leaves = re.search(r"(?m)^LEAVES=\(([^)]*)\)", text)
        assert leaves is not None
        names = set(self.crates)
        for name in leaves.group(1).split():
            with self.subTest(crate=name):
                self.assertEqual(_internal_deps(self.crates[name], names), set())


class NpmReleaseGateTests(unittest.TestCase):
    def test_the_published_bytes_pass_the_suite_before_either_publish(self) -> None:
        text = (release.ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
        job = text.split("\n  npm:\n", 1)[1].split("\n  pypi:\n", 1)[0]

        self.assertNotIn("wasm-pack build", job)
        main_steps = [
            "node scripts/check-wasm-bundle.mjs",
            "npm ci",
            "npm test",
            "npm run smoke-install",
            "npm publish --access public",
        ]
        positions = [job.find(step) for step in main_steps]
        self.assertNotIn(-1, positions)
        self.assertEqual(positions, sorted(positions))
        mcp = job.split("sharpebench-mcp (skip if version exists)", 1)[1]
        self.assertLess(mcp.index("npm test"), mcp.index("npm publish --access public"))


class ArenaClockRaceTests(unittest.TestCase):
    """The daily clock commits to main; it must not be able to harm a release."""

    @staticmethod
    def _commit(root: Path, path: str, text: str, message: str) -> str:
        (root / path).parent.mkdir(parents=True, exist_ok=True)
        (root / path).write_text(text, encoding="utf-8")
        release.run(root, "git", "add", "--", path)
        release.git(root, "commit", "--quiet", "-m", message)
        return release.git(root, "rev-parse", "HEAD")

    def setUp(self) -> None:
        holder = tempfile.TemporaryDirectory(prefix="sharpebench-clock-")
        self.addCleanup(holder.cleanup)
        base = Path(holder.name)
        self.origin = base / "origin.git"
        self.origin.mkdir()
        release.run(self.origin, "git", "init", "--quiet", "--bare", "--initial-branch=main")
        self.tree = base / "tree"
        self.clock = base / "clock"
        self.tree.mkdir()
        release.run(self.tree, "git", "init", "--quiet", "--initial-branch=main")
        release.run(self.tree, "git", "remote", "add", "origin", str(self.origin))
        self._commit(self.tree, "arena/state.json", "{}\n", "initial")
        release.run(self.tree, "git", "push", "--quiet", "origin", "HEAD:main")
        release.run(base, "git", "clone", "--quiet", str(self.origin), str(self.clock))

    def _clock_tick(self) -> None:
        self._commit(self.clock, "arena/state.json", '{"t":1}\n', "chore(arena): tick")
        release.run(self.clock, "git", "push", "--quiet", "origin", "HEAD:main")

    def test_a_clock_commit_during_the_cut_refuses_commits_and_tag_together(self) -> None:
        self._commit(self.tree, "release.txt", "cut\n", "release commit")
        release.git(self.tree, "tag", "-a", "v9.9.9", "-m", "candidate")
        self._clock_tick()

        with self.assertRaises(release.ReleaseError):
            release.push_release(self.tree, "v9.9.9")

        self.assertEqual(
            release.git(self.origin, "tag", "--list"), "", "the tag reached origin alone"
        )
        self.assertNotIn(
            "release commit", release.git(self.origin, "log", "--format=%s", "main")
        )

    def test_a_clock_commit_after_the_push_leaves_the_tag_on_main(self) -> None:
        tagged = self._commit(self.tree, "release.txt", "cut\n", "release commit")
        release.git(self.tree, "tag", "-a", "v9.9.9", "-m", "candidate")
        release.push_release(self.tree, "v9.9.9")
        release.run(self.clock, "git", "pull", "--quiet", "--ff-only", "origin", "main")
        self._clock_tick()

        release.run(self.tree, "git", "fetch", "--quiet", "origin", "main:refs/remotes/origin/main")
        problems, commit = release.verify_tag(self.tree, "v9.9.9")

        self.assertEqual(commit, tagged)
        self.assertFalse([p for p in problems if "origin/main" in p], problems)

    def test_the_clock_state_file_stays_outside_the_provenance_scope(self) -> None:
        spec = importlib.util.spec_from_file_location(
            "provenance_common", release.ROOT / "paper/src/provenance_common.py"
        )
        assert spec is not None and spec.loader is not None
        common = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(common)
        scope = (*common.SOURCE_SCOPE, *common.ARTIFACT_SCOPE)
        self.assertNotIn("arena/state.json", scope)
        self.assertFalse([p for p in scope if p in ("arena/*", "arena/**", "arena/**/*")])


if __name__ == "__main__":
    unittest.main()
