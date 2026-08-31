// Pack-then-offline-install smoke test for the published artifact.
//
// `npm test` exercises the working tree; nothing exercised the *tarball*. A
// `files` list that drops `dist` or `pkg`, a `main` that points at a file the
// pack excludes, or a wasm loader path that only resolves in-repo all pass the
// unit tests and then break for every installer. This script tests exactly what
// an installer gets: `npm pack` -> a throwaway project -> an OFFLINE install
// from the tarball with an isolated cache (so the registry cannot paper over a
// missing file) -> `require()` -> assert a real export works.
//
// npm is spawned as `node npm-cli.js ...`: on Node >= 20.12 `execFileSync`
// refuses `npm.cmd` on Windows without `shell: true` (CVE-2024-27980), and a
// shell means quoting; the JS entry point needs neither.

import assert from "node:assert";
import { execFileSync } from "node:child_process";
import { cpSync, existsSync, mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const packageDir = path.dirname(path.dirname(fileURLToPath(import.meta.url)));

function npmCliPath() {
  const nodeDir = path.dirname(process.execPath);
  const candidates = [
    // Windows: npm ships beside node.exe.
    path.join(nodeDir, "node_modules", "npm", "bin", "npm-cli.js"),
    // Unix: node lives in <prefix>/bin, npm in <prefix>/lib/node_modules.
    path.join(nodeDir, "..", "lib", "node_modules", "npm", "bin", "npm-cli.js"),
  ];
  const found = candidates.find(existsSync);
  assert(found, `cannot find npm-cli.js near ${process.execPath}; tried:\n${candidates.join("\n")}`);
  return found;
}

const npmCli = npmCliPath();
function npm(args, options) {
  return execFileSync(process.execPath, [npmCli, ...args], {
    encoding: "utf8",
    stdio: ["ignore", "pipe", "inherit"],
    ...options,
  });
}

// The pack ships prebuilt artifacts and runs no build of its own (no prepack
// script), so an unbuilt tree must be a loud refusal here, not an empty pack
// that "installs" fine.
for (const artifact of ["dist/index.js", "pkg/sharpebench.js", "pkg/sharpebench_bg.wasm"]) {
  assert(
    existsSync(path.join(packageDir, artifact)),
    `${artifact} is missing: build first (npm run build; the pkg/ wasm comes from wasm-pack)`,
  );
}

const workDir = mkdtempSync(path.join(tmpdir(), "sharpebench-smoke-"));
try {
  // Pack from a staged copy in publish-time state. wasm-pack drops a
  // `.gitignore` containing `*` into pkg/, and npm honors a nested .gitignore
  // even for a directory the `files` list names, so packing the working tree
  // as-is ships no wasm. At release `prepublishOnly` deletes that file before
  // `npm publish`; the stage mirrors exactly that (and skips node_modules,
  // which the pack never reads).
  const stageDir = path.join(workDir, "stage");
  cpSync(packageDir, stageDir, {
    recursive: true,
    filter: (source) => {
      const relative = path.relative(packageDir, source);
      return relative !== "node_modules" && relative !== path.join("pkg", ".gitignore");
    },
  });
  const packed = JSON.parse(
    npm(["pack", "--json", "--pack-destination", workDir], { cwd: stageDir }),
  );
  assert.strictEqual(packed.length, 1, `npm pack must produce exactly one tarball, got ${packed.length}`);
  const [tarball] = packed;
  assert(tarball.files.length > 0, "npm pack produced an empty tarball");
  const packedPaths = new Set(tarball.files.map((file) => file.path));
  for (const artifact of ["package.json", "dist/index.js", "pkg/sharpebench.js", "pkg/sharpebench_bg.wasm"]) {
    assert(packedPaths.has(artifact), `${artifact} is not in the tarball; check the files list in package.json`);
  }
  const tarballPath = path.join(workDir, tarball.filename);
  assert(existsSync(tarballPath), `npm pack reported ${tarball.filename} but it is not on disk`);

  const projectDir = path.join(workDir, "project");
  mkdirSync(projectDir);
  writeFileSync(
    path.join(projectDir, "package.json"),
    JSON.stringify({ name: "sharpebench-smoke", private: true }),
  );
  // --offline + a fresh empty cache: everything must come from the tarball.
  npm(
    [
      "install",
      "--offline",
      "--no-audit",
      "--no-fund",
      "--cache",
      path.join(workDir, "npm-cache"),
      tarballPath,
    ],
    { cwd: projectDir },
  );

  // Not just "require resolves": call into the wasm kernel, so a tarball whose
  // JS shims packed but whose .wasm did not still fails here.
  const probe = `
    const bench = require("@general-liquidity/sharpebench");
    if (typeof bench.score !== "function") throw new Error("score is not exported");
    const composite = bench.score([{ agent_id: "smoke", runs: [{ returns: [0.01, -0.005, 0.02] }], in_sample_trials: 1 }]);
    if (!Array.isArray(composite) || composite.length !== 1) throw new Error("score(field) must return one row");
    console.log("smoke-install ok: packed tarball installs offline and the wasm kernel answers");
  `;
  const output = execFileSync(process.execPath, ["-e", probe], {
    cwd: projectDir,
    encoding: "utf8",
  });
  process.stdout.write(output);
} finally {
  rmSync(workDir, { recursive: true, force: true, maxRetries: 5 });
}
