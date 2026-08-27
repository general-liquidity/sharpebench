import test from "node:test";
import assert from "node:assert";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

// The MCP package is published in the same release run that publishes its own
// dependency, @general-liquidity/sharpebench. Before that dependency version
// exists on the registry, the committed lock cannot carry the integrity hash of
// the tarball npm will actually serve for it. npm accepts the fresh metadata and
// then rejects the tarball against the stale pre-release integrity, which fails
// the publish step. Installing without consulting the lock is what makes the
// release work; this test fails if that flag is dropped again.
const workflow = readFileSync(
  fileURLToPath(new URL("../../../.github/workflows/release.yml", import.meta.url)),
  "utf8",
);

// The step body with YAML comments dropped: the comments quote the very commands
// under test, so a scan over the raw text would match prose instead of script.
function mcpPublishStep() {
  const start = workflow.indexOf("- name: Publish @general-liquidity/sharpebench-mcp");
  assert.notStrictEqual(start, -1, "release.yml has no MCP publish step");
  const rest = workflow.slice(start + 1);
  const end = rest.indexOf("\n      - name: ");
  const body = end === -1 ? rest : rest.slice(0, end);
  return body
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line !== "" && !line.startsWith("#"));
}

test("the MCP publish step installs without consulting the committed lock", () => {
  const installs = mcpPublishStep().filter((line) => /^npm (install|ci)\b/.test(line));

  assert.deepStrictEqual(
    installs,
    ["npm install --package-lock=false"],
    "the MCP publish step must install exactly once, with --package-lock=false, so " +
      "the just-published dependency resolves from the registry rather than against " +
      "the pre-release integrity hash in npm/mcp/package-lock.json",
  );
});

test("the MCP publish step waits for its dependency to propagate first", () => {
  const lines = mcpPublishStep();
  const wait = lines.findIndex((line) =>
    line.includes('npm view "@general-liquidity/sharpebench@$DEPV" version'),
  );
  const install = lines.findIndex((line) => /^npm install\b/.test(line));
  assert.notStrictEqual(wait, -1, "the registry read-path wait loop is gone");
  assert.notStrictEqual(install, -1, "the step no longer installs");
  assert.ok(wait < install, "the propagation wait must come before the install");
});
