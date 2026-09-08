#!/usr/bin/env python3
"""Build a fresh downstream crate against the normalized `cargo package` output.

`cargo test --workspace` compiles the source tree, where every intra-workspace
dependency is a path. What a consumer downloads is different: Cargo rewrites the
manifest, applies the include/exclude rules, and the path links are gone. A file that
never reached the archive, a `mod` that only resolved through the workspace, or a
dependency edge that only existed as a path all pass the source-tree suite and then fail
for the first person who runs `cargo add`.

This packages the crate and its two workspace dependencies, then compiles and RUNS a
throwaway crate that has never seen this repository and depends only on those archives.
`[patch.crates-io]` redirects the packaged manifest's registry dependencies at the
sibling archives, so the check needs no publication and no network resolution of this
version: the code under test is the packaged bytes, start to finish.

The asserted value is the same zero-volatility quote the packed npm tarball asserts in
`npm/test/smoke-install.mjs`, so the Rust and WebAssembly consumer legs are pinned to one
number rather than to two independently drifting ones.
"""

import argparse
import json
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import tomllib

# The crate a consumer actually adds, followed by the workspace crates its packaged
# manifest names as registry dependencies.
TARGET = "sharpebench-core"
SIBLINGS = ("sharpebench-protocol", "sharpebench-stats")

CONSUMER_MAIN = """\
use sharpebench_core::{bs_price, pass_k, PassMode};

fn main() {
    // Identical to the assertion the packed npm tarball makes on the same inputs.
    let price = bs_price(100.0, 100.0, 1.0, 0.05, 0.0, true).expect("finite inputs are accepted");
    assert!(
        (price - 4.877057549928594).abs() < 1e-12,
        "packaged zero-volatility quote drifted: {price}"
    );
    assert!(
        bs_price(100.0, 100.0, 1.0, 0.05, -0.1, true).is_err(),
        "packaged crate accepted a negative volatility"
    );
    assert!(pass_k(&[true, true, true], PassMode::All));
    assert!(!pass_k(&[true, false, true], PassMode::All));
    assert!(pass_k(&[true, false, true], PassMode::AtLeast(2)));
    println!("packaged consumer built against the archive and ran");
}
"""


def package(root: Path, crates: tuple[str, ...], allow_dirty: bool) -> None:
    command = ["cargo", "package", "--no-verify", "--target-dir", str(root / "target")]
    for crate in crates:
        command += ["-p", crate]
    if allow_dirty:
        command.append("--allow-dirty")
    # --no-verify: the verification build resolves the packaged manifest's dependencies
    # from the registry, which fails for a version this repository has not published yet.
    # The consumer build below is a stricter check of the same archive.
    subprocess.run(command, cwd=root, check=True)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--allow-dirty", action="store_true", help="package uncommitted changes")
    args = parser.parse_args()

    root = Path(__file__).resolve().parents[1]
    version = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))["workspace"][
        "package"
    ]["version"]
    crates = (TARGET, *SIBLINGS)
    package(root, crates, args.allow_dirty)

    with tempfile.TemporaryDirectory(prefix="sharpebench-consumer-") as work:
        # Unpack the .crate archives rather than reusing cargo's staging directory: the
        # archive is the artifact a registry serves, and extracting it is what proves the
        # build below reads packaged bytes and not a leftover working tree.
        extracted = Path(work) / "archives"
        extracted.mkdir()
        packaged = {}
        for crate in crates:
            archive = root / "target" / "package" / f"{crate}-{version}.crate"
            if not archive.is_file():
                raise SystemExit(f"cargo package produced no archive for {crate} {version}")
            with tarfile.open(archive, "r:gz") as tar:
                tar.extractall(extracted, filter="data")
            directory = extracted / f"{crate}-{version}"
            if not (directory / "Cargo.toml").is_file():
                raise SystemExit(f"{archive.name} does not contain {crate}-{version}/Cargo.toml")
            packaged[crate] = directory

        consumer = Path(work) / "consumer"
        (consumer / "src").mkdir(parents=True)
        patches = "\n".join(
            f'{crate} = {{ path = {json.dumps(str(packaged[crate]))} }}' for crate in SIBLINGS
        )
        (consumer / "Cargo.toml").write_text(
            "[package]\n"
            'name = "packaged-consumer"\n'
            'version = "0.0.0"\n'
            'edition = "2021"\n'
            "publish = false\n\n"
            "# Empty table: this crate is deliberately not a member of any workspace.\n"
            "[workspace]\n\n"
            "[dependencies]\n"
            f"{TARGET} = {{ path = {json.dumps(str(packaged[TARGET]))} }}\n\n"
            "[patch.crates-io]\n"
            f"{patches}\n",
            encoding="utf-8",
        )
        (consumer / "src" / "main.rs").write_text(CONSUMER_MAIN, encoding="utf-8")
        result = subprocess.run(
            ["cargo", "run", "--quiet"],
            cwd=consumer,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
        )
        print(result.stdout, end="")
        if result.returncode != 0:
            raise SystemExit(f"the packaged-archive consumer failed with {result.returncode}")
        if "packaged consumer built against the archive and ran" not in result.stdout:
            raise SystemExit("the consumer binary did not report a completed run")


if __name__ == "__main__":
    main()
    sys.exit(0)
