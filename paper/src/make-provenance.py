"""Write a deterministic content manifest for the paper's result artifacts."""

from __future__ import annotations

import itertools
import json
import os
import subprocess
import sys
from pathlib import Path

from provenance_common import (
    ARTIFACT_SCOPE,
    EXCLUDED_ARTIFACT_PREFIXES,
    EXCLUDED_DIR_NAMES,
    NOTE,
    REPRODUCTION_ENTRYPOINT,
    SCHEMA_VERSION,
    SOURCE_SCOPE,
    digest_bytes,
    matching_files,
    snapshot_digest,
)


ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "paper" / "evidence" / "provenance.json"

def sha256(path: Path, *, canonical_text: bool = False) -> str:
    return digest_bytes(path.read_bytes(), canonical_text=canonical_text)


def match(pattern: str) -> list[Path]:
    return matching_files(ROOT, pattern, frozenset(EXCLUDED_DIR_NAMES))


def files(patterns: tuple[str, ...]) -> list[Path]:
    """Expand every pattern, refusing any that matches nothing.

    A scope entry that resolves to zero files is either dead or misspelled, and
    the manifest cannot tell you which: it records the pattern, records no file
    against it, and validates.  The entire scope could go empty this way and
    every digest check would still pass, because there would be nothing left to
    check.  So an empty expansion fails at generation time, while the person who
    moved the directory is still standing there.
    """
    empty = [pattern for pattern in patterns if not match(pattern)]
    if empty:
        print(
            "scope patterns match no files: "
            + ", ".join(empty)
            + "\nRemove the dead pattern or fix the path; a scope that expands to "
            "nothing verifies nothing.",
            file=sys.stderr,
        )
        raise SystemExit(2)
    found: set[Path] = set()
    for pattern in patterns:
        found.update(match(pattern))
    return sorted(found, key=lambda path: path.as_posix())


source_paths = files(SOURCE_SCOPE)
source_records = [
    {
        "path": path.relative_to(ROOT).as_posix(),
        "sha256": sha256(path, canonical_text=True),
    }
    for path in source_paths
]
snapshot = snapshot_digest(source_records)

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
    "schema_version": SCHEMA_VERSION,
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
    "reproduction_entrypoint": REPRODUCTION_ENTRYPOINT,
    "note": NOTE,
    "source_files": source_records,
    "artifacts": artifacts,
}
_TEMP_SEQUENCE = itertools.count()


def write_atomically(path: Path, payload: bytes) -> None:
    """Replace `path` with `payload`, or leave it exactly as it was.

    A plain open-and-write leaves a window in which the manifest on disk is a
    truncated prefix of the new one, so an interrupted or out-of-space run turns
    the file every provenance check is measured against into one that describes
    no tree at all.  Instead: write a private temp file in the destination
    directory, fsync it so its bytes are durable before anything points at them,
    rename over the target (atomic within a directory), then fsync the directory
    so the rename itself survives a crash.  O_EXCL with a pid- and
    sequence-qualified name means two concurrent runs cannot share a temp file.
    """
    directory = path.parent
    temp = directory / f".{path.name}.{os.getpid()}.{next(_TEMP_SEQUENCE)}.tmp"
    descriptor = os.open(temp, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o644)
    try:
        with os.fdopen(descriptor, "wb") as handle:
            handle.write(payload)
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temp, path)
    except BaseException:
        temp.unlink(missing_ok=True)
        raise
    # Windows exposes no directory handle to sync; the rename is durable there.
    if hasattr(os, "O_DIRECTORY"):
        directory_descriptor = os.open(directory, os.O_RDONLY | os.O_DIRECTORY)
        try:
            os.fsync(directory_descriptor)
        finally:
            os.close(directory_descriptor)


# The payload is encoded here rather than written in text mode, so the manifest
# is the same bytes on Windows as on CI and "regenerate and diff" is a byte-wise
# check everywhere rather than only where the checkout convention is LF.
write_atomically(
    OUT, (json.dumps(manifest, indent=2, sort_keys=True) + "\n").encode("utf-8")
)
print(
    f"wrote {OUT.relative_to(ROOT)}: {len(source_records)} sources, "
    f"{len(artifacts)} artifacts"
)
