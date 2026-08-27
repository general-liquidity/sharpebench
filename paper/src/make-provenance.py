"""Write a deterministic content manifest for the paper's result artifacts."""

from __future__ import annotations

import hashlib
import json
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "paper" / "evidence" / "provenance.json"

SOURCE_SCOPE = (
    "Cargo.toml",
    "Cargo.lock",
    "crates/**/*.toml",
    "crates/**/*.rs",
    "crates/**/schema/*.json",
    "paper/src/*.py",
    "paper/evidence/*.py",
    "paper/main.tex",
    "paper/sections/*.tex",
    "paper/refs.bib",
    "data/*.csv",
    "arena/**/*.json",
    "arena/**/*.toml",
)
ARTIFACT_SCOPE = ("paper/evidence/final/*.jsonl", "paper/figures/*.pdf")
EXCLUDED_DIR_NAMES = ("target", ".venv", "__pycache__", "node_modules", ".git")
EXCLUDED_ARTIFACT_PREFIXES = ("llm-cache-", "llm-field-")


def sha256(path: Path, *, canonical_text: bool = False) -> str:
    data = path.read_bytes()
    if canonical_text:
        # Git may materialize the same text blob as LF on CI and CRLF on a
        # Windows worktree.  Source identity follows the repository text, not
        # the checkout convention; result artifacts remain byte-exact below.
        data = data.replace(b"\r\n", b"\n")
    return hashlib.sha256(data).hexdigest()


def files(patterns: tuple[str, ...]) -> list[Path]:
    found: set[Path] = set()
    for pattern in patterns:
        found.update(
            path
            for path in ROOT.glob(pattern)
            if path.is_file()
            and not any(
                part in EXCLUDED_DIR_NAMES for part in path.relative_to(ROOT).parts
            )
        )
    return sorted(found, key=lambda path: path.as_posix())


source_paths = files(SOURCE_SCOPE)
source_records = [
    {
        "path": path.relative_to(ROOT).as_posix(),
        "sha256": sha256(path, canonical_text=True),
    }
    for path in source_paths
]
snapshot = hashlib.sha256(
    "".join(f"{item['sha256']}  {item['path']}\n" for item in source_records).encode()
).hexdigest()

artifact_paths = [
    path
    for path in files(ARTIFACT_SCOPE)
    if not path.name.startswith(EXCLUDED_ARTIFACT_PREFIXES)
]
artifacts = [
    {"path": path.relative_to(ROOT).as_posix(), "sha256": sha256(path)}
    for path in artifact_paths
]

head = subprocess.check_output(
    ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True
).strip()
dirty = bool(
    subprocess.check_output(
        ["git", "status", "--porcelain"], cwd=ROOT, text=True
    ).strip()
)
manifest = {
    "schema_version": 3,
    # A manifest cannot contain the hash of the commit that will contain the
    # manifest without becoming self-referential.  Record the checked-out base
    # commit honestly; source_snapshot_sha256 binds the candidate's actual
    # (possibly not-yet-committed) source bytes.
    "generated_at_head": head,
    "generated_at_head_dirty": dirty,
    "source_snapshot_sha256": snapshot,
    "source_snapshot_scope": list(SOURCE_SCOPE),
    "source_snapshot_excludes": list(EXCLUDED_DIR_NAMES),
    "artifact_scope": list(ARTIFACT_SCOPE),
    "artifact_excluded_prefixes": list(EXCLUDED_ARTIFACT_PREFIXES),
    "reproduction_entrypoint": "commands in paper/sections/A-commands.tex",
    "note": "Incomplete paid LLM runs are excluded from result provenance.",
    "source_files": source_records,
    "artifacts": artifacts,
}
# newline="" suppresses the platform translation Path.write_text applies, so the
# manifest is the same bytes on Windows as on CI and "regenerate and diff" is a
# byte-wise check everywhere rather than only where the checkout convention is LF.
with OUT.open("w", encoding="utf-8", newline="") as handle:
    handle.write(json.dumps(manifest, indent=2, sort_keys=True) + "\n")
print(
    f"wrote {OUT.relative_to(ROOT)}: {len(source_records)} sources, "
    f"{len(artifacts)} artifacts"
)
