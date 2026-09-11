#!/usr/bin/env node
// The committed npm WASM bundle (`npm/pkg`) is a checked-in build artifact: check that it
// still answers like the bundle this source tree builds.
//
// Nothing in this repository compared the two. CI (`npm.yml`) and the release workflow each
// run `wasm-pack build` into `npm/pkg` before testing or publishing, so both always test a
// bundle they just built and never look at the committed one. The committed copy therefore
// drifted to `sharpebench-stats/0.21.0`, three minor versions behind the workspace, with
// every gate green; the first thing to notice was a working-tree `npm test` run by hand.
//
// WHAT THIS ASSERTS, AND WHY IT IS NOT BYTE EQUALITY.
//
// The bundle's bytes are not a stable function of the source. wasm-pack output differs
// across operating systems on the same pinned toolchain, and inside one host it moves for
// changes that cannot alter behaviour: the binary embeds `panic::Location` records, so a
// docs-only edit that added three lines to `crates/sharpebench-core/src/composite.rs` moved
// 22 bytes of line numbers in it (`0x07b9` -> `0x07bc`, three records pointing at that
// 40-character path) at an unchanged total size. A byte-equality gate would go red for
// that, which is a gate nobody keeps.
//
// What is asserted instead is DIFFERENTIAL BEHAVIORAL EQUIVALENCE: both bundles are loaded
// and driven through every export on a fixed battery of deterministic inputs, and their
// returned strings are compared byte for byte. That is host-independent and immune to a
// line-number shift, and it covers the whole exported surface rather than a stamp. The
// committed bundle is separately anchored against the two committed golden score files,
// which is an absolute check rather than a comparison against a peer: the Rust parity test
// pins the *source-built* facade to those goldens, and this pins the *shipped bytes* to
// them.
//
// WHAT THIS NO LONGER CATCHES. Two bundles agreeing on every input in the battery are taken
// as equivalent, so a divergence only reachable by an input the battery does not contain
// survives. Byte equality would have caught that and this does not. The battery is the
// gate's real scope: extend it when an export grows a branch. What is enforced is that it
// drives every export the built module has, read off that module rather than a list kept
// here, so a new export fails this gate until the battery covers it. Byte equality is still
// computed and reported, so a bundle that does match is said to match, but a mismatch is
// reported as an environment or line-record difference rather than failed.
//
// A failure is classified before it is reported, because a stale committed artifact and a
// build that diverges for another reason produce the same inequality and need different
// remedies.
//
// Usage: node scripts/check-wasm-bundle.mjs [bundle dir]   (from the repository root)
//
// The optional argument names the bundle to check instead of `npm/pkg`. CI passes nothing;
// it exists so the classification can itself be checked against a known-stale bundle
// without touching the worktree.

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { createRequire } from "node:module";
import { mkdtempSync, readFileSync, readdirSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

const REPO = process.cwd();
const COMMITTED = path.resolve(REPO, process.argv[2] ?? path.join("npm", "pkg"));
const require_ = createRequire(import.meta.url);

// wasm-pack writes a `.gitignore` that is never committed, so it is not part of the
// comparison; every other file it emits is.
const IGNORED = new Set([".gitignore"]);

const sha256 = (file) => createHash("sha256").update(readFileSync(file)).digest("hex");

function digests(dir) {
  const out = new Map();
  for (const name of readdirSync(dir).sort()) {
    if (IGNORED.has(name)) continue;
    out.set(name, sha256(path.join(dir, name)));
  }
  return out;
}

const read = (rel) => readFileSync(path.join(REPO, rel), "utf8");

/**
 * Strip insignificant whitespace from pretty JSON without parsing floats, so a golden is
 * compared as bytes rather than through a parse and re-serialise that could hide a 1-ULP
 * difference. String contents are preserved verbatim. Same rule as `minify` in
 * `crates/sharpebench-wasm/tests/native_parity.rs`.
 */
function minify(pretty) {
  let out = "";
  let inString = false;
  let escaped = false;
  for (const c of pretty) {
    if (inString) {
      out += c;
      if (escaped) escaped = false;
      else if (c === "\\") escaped = true;
      else if (c === '"') inString = false;
    } else if (c === '"') {
      inString = true;
      out += c;
    } else if (!/\s/.test(c)) {
      out += c;
    }
  }
  return out;
}

/** The committed golden fields and the score files they must reproduce. */
const GOLDENS = [
  {
    name: "suites/example_submissions.json",
    field: "suites/example_submissions.json",
    scores: "crates/sharpebench-core/golden/example_submissions.scores.json",
  },
  {
    name: "golden/synthetic_field.input.json",
    field: "crates/sharpebench-core/golden/synthetic_field.input.json",
    scores: "crates/sharpebench-core/golden/synthetic_field.scores.json",
  },
];

const RETURNS = Array.from({ length: 60 }, (_, i) => 0.002 + 0.0005 * ((i % 3) - 1));
const WAVE = Array.from({ length: 120 }, (_, i) => 0.02 + 0.1 * Math.sin(i * 0.9));
const FIELD = JSON.stringify([
  { agent_id: "strong", runs: [{ returns: RETURNS }] },
  { agent_id: "weak", runs: [{ returns: RETURNS.map((x) => x - 0.0021) }] },
]);

/**
 * The fixed input battery: every export, on inputs chosen to reach the branches a version
 * stamp does not cover, including the refusal paths (a malformed dispersion, an invalid
 * frequency, a misaligned regime array), which are part of the surface a consumer sees.
 * Every entry is deterministic and takes no clock, no randomness and no file system.
 * `label` is what a failure names, so it has to say which call diverged.
 */
function battery() {
  const calls = [];
  const push = (label, fn, args) => calls.push({ label, fn, args });

  push("self_audit", "self_audit", []);
  for (const seed of ["scenario-1", "", "éé", "0".repeat(64)]) {
    push(`canary(${JSON.stringify(seed)})`, "canary", [seed]);
  }

  for (const g of GOLDENS) {
    push(`score(golden ${g.name})`, "score", [read(g.field), ""]);
  }
  push("score(field)", "score", [FIELD, ""]);
  push("score(field, n_trials 500)", "score", [FIELD, '{"n_trials":500,"trials_sr_std":0.5,"dsr_bar":0.95,"per_run_psr_bar":0.9,"alpha":0.05,"bootstrap_seed":7,"n_boot":99,"block_prob":0.1}']);
  push("score(empty field)", "score", ["[]", ""]);
  push("score(malformed)", "score", ["{", ""]);
  push("score_agent", "score_agent", [JSON.stringify({ agent_id: "strong", runs: [{ returns: RETURNS }] }), ""]);
  push("score_agent(wave, 500 trials)", "score_agent", [
    JSON.stringify({ agent_id: "wave", runs: [{ returns: WAVE }] }),
    '{"n_trials":500,"trials_sr_std":0.5,"dsr_bar":0.95,"per_run_psr_bar":0.9,"alpha":0.05,"bootstrap_seed":7,"n_boot":99,"block_prob":0.1}',
  ]);
  push("score_agent(unknown field)", "score_agent", ['{"agent_id":"x","runs":[],"nope":1}', ""]);

  for (const config of [
    '{"n_trials":1}',
    '{"n_trials":20}',
    '{"n_trials":20,"periods_per_year":52}',
    '{"n_trials":20,"periods_per_year":8760}',
    '{"n_trials":20,"sr_benchmark":1.0,"periods_per_year":252}',
    '{"n_trials":500,"trials_sr_std":0.5}',
    '{"n_trials":20,"trials_sr_std":-0.5}',
    '{"n_trials":20,"periods_per_year":0}',
    '{"n_trials":1000}',
  ]) {
    push(`is_my_sharpe_real(${config})`, "is_my_sharpe_real", [JSON.stringify(RETURNS), config]);
  }
  push("is_my_sharpe_real(overflow)", "is_my_sharpe_real", ["[1e308,1e308,1e308]", '{"n_trials":2}']);
  push("is_my_sharpe_real(empty)", "is_my_sharpe_real", ["[]", '{"n_trials":2}']);

  push("is_my_sharpe_real_full(pair)", "is_my_sharpe_real_full", [
    JSON.stringify([RETURNS, RETURNS.map((x) => x - 0.0001)]),
    0,
    '{"n_trials":2}',
  ]);
  push("is_my_sharpe_real_full(wave field)", "is_my_sharpe_real_full", [
    JSON.stringify([WAVE, RETURNS, RETURNS.map((x) => -x)]),
    0,
    '{"n_trials":5}',
  ]);
  push("is_my_sharpe_real_full(overflow)", "is_my_sharpe_real_full", [
    "[[1e308,1e308],[1e308,1e308]]",
    0,
    '{"n_trials":2}',
  ]);

  for (const vol of [0.2, 0, 1.5]) {
    for (const is_call of [true, false]) {
      const params = JSON.stringify({ spot: 100, strike: 100, t_years: 1, rate: 0.05, vol, is_call });
      push(`greeks(vol ${vol}, call ${is_call})`, "greeks", [params]);
    }
  }
  push("greeks(deep itm)", "greeks", ['{"spot":180,"strike":100,"t_years":0.25,"rate":0.03,"vol":0.35,"is_call":true}']);
  push("greeks(t_years 0)", "greeks", ['{"spot":100,"strike":100,"t_years":0,"rate":0.05,"vol":0.2,"is_call":true}']);
  push("greeks(negative vol)", "greeks", ['{"spot":100,"strike":100,"t_years":1,"rate":0.05,"vol":-0.2,"is_call":true}']);

  push("audit_briefing(empty)", "audit_briefing", ['{"sections":[]}', ""]);
  const section = (asset_area) => ({
    asset_area,
    rows: [
      { text: "observable", kind: "fact" },
      { text: "uncertainty", kind: "uncertainty" },
    ],
  });
  push("audit_briefing(overweight)", "audit_briefing", [
    JSON.stringify({ sections: [section(" Energy "), section("ENERGY"), section("energy"), section("rates")] }),
    "",
  ]);
  push("audit_briefing(policy)", "audit_briefing", [
    JSON.stringify({ sections: [section("energy")] }),
    '{"max_area_salience":1}',
  ]);
  push("audit_briefing(unspecified ordering)", "audit_briefing", [
    JSON.stringify({
      sections: [section("energy"), section("rates")],
      return_table: { ordering: "unspecified", entries: [] },
    }),
    "",
  ]);

  push("score_allocation(single)", "score_allocation", ['{"steps":[{"weights":[1.0]}]}', ""]);
  push("score_allocation(turnover)", "score_allocation", [
    '{"steps":[{"weights":[0.5,0.5]},{"weights":[0.1,0.9]},{"weights":[0.9,0.1]}]}',
    "",
  ]);
  push("score_allocation(malformed)", "score_allocation", ['{"steps":[{"weights":[2.0]}]}', ""]);

  push("decompose_uncertainty(binary)", "decompose_uncertainty", ['{"outcomes":[1,0,1]}']);
  push("decompose_uncertainty(bools)", "decompose_uncertainty", ['{"outcomes":[true,false,true]}']);
  push("decompose_uncertainty(nonbinary)", "decompose_uncertainty", ['{"outcomes":[0,0.5,1]}']);

  push("percentile_selection(mean_return)", "percentile_selection", [
    JSON.stringify([RETURNS, WAVE, RETURNS.map((x) => -x)]),
    '{"n_boot":50,"seed":3}',
  ]);
  push("percentile_selection(sharpe)", "percentile_selection", [
    JSON.stringify([RETURNS, WAVE]),
    '{"utility":"sharpe","n_boot":50,"seed":3,"alpha":0.1}',
  ]);
  push("percentile_selection(empty candidate)", "percentile_selection", [
    JSON.stringify([[], [-0.02, -0.01, -0.03]]),
    '{"n_boot":10}',
  ]);
  push("percentile_selection(bad utility)", "percentile_selection", [JSON.stringify([RETURNS]), '{"utility":"nope"}']);

  for (const adoption of [0, 0.25, 0.9, 1]) {
    push(`crowding_half_life(${adoption})`, "crowding_half_life", [
      JSON.stringify({ adoption, theta: 0.4, delta_max: 0.8, curvature: 1.5 }),
    ]);
  }
  push("crowding_half_life(missing field)", "crowding_half_life", ['{"adoption":0.5}']);

  push("classify_disqualification(field)", "classify_disqualification", [FIELD, ""]);
  push("classify_disqualification(bad ci level)", "classify_disqualification", [
    FIELD,
    '{"n_trials":2,"trials_sr_std":0.5,"dsr_bar":0.95,"per_run_psr_bar":0.9,"alpha":0.05,"bootstrap_seed":7,"n_boot":99,"block_prob":0.1,"dsr_ci_level":1.5}',
  ]);

  const regimes = JSON.stringify(["calm", "calm", "stress", "stress"]);
  push("regime_compare(reversal)", "regime_compare", [
    "[0.02,0.03,-0.01,-0.02]",
    "[0.0,0.01,0.01,0.02]",
    regimes,
    '{"min_periods":2}',
  ]);
  push("regime_compare(defaults)", "regime_compare", [
    "[0.02,0.03,-0.01,-0.02]",
    "[0.0,0.01,0.01,0.02]",
    regimes,
    "",
  ]);
  push("regime_compare(misaligned)", "regime_compare", ["[0.1]", "[0.1,0.2]", '["calm"]', ""]);

  return calls;
}

/** Every answer a bundle gives, as `label -> returned string`. */
function interrogate(dir) {
  const kernel = require_(path.join(dir, "sharpebench.js"));
  const answers = new Map();
  for (const { label, fn, args } of battery()) {
    if (typeof kernel[fn] !== "function") {
      answers.set(label, "<export absent>");
      continue;
    }
    // A refusal is an answer: the exports wrap errors as `{"error":...}` rather than
    // throwing, but a panic in the module would throw and must not abort the comparison.
    try {
      answers.set(label, kernel[fn](...args));
    } catch (e) {
      answers.set(label, `<threw ${e && e.message ? e.message : String(e)}>`);
    }
  }
  let methodologyVersion = null;
  try {
    methodologyVersion = JSON.parse(kernel.is_my_sharpe_real(JSON.stringify(RETURNS), '{"n_trials":2}'))
      .methodology_version;
  } catch {
    methodologyVersion = null;
  }
  const goldens = GOLDENS.map((g) => {
    let ok = false;
    try {
      ok = kernel.score(read(g.field), "") === minify(read(g.scores));
    } catch {
      ok = false;
    }
    return { name: g.name, ok };
  });
  return { answers, methodologyVersion, goldens };
}

const name = path.relative(REPO, COMMITTED) || COMMITTED;
const workDir = mkdtempSync(path.join(tmpdir(), "sharpebench-bundle-"));
try {
  const fresh = path.join(workDir, "pkg");
  execFileSync(
    "wasm-pack",
    ["build", "crates/sharpebench-wasm", "--target", "nodejs", "--out-dir", fresh, "--out-name", "sharpebench"],
    { cwd: REPO, stdio: ["ignore", "inherit", "inherit"] },
  );

  const committedDigests = digests(COMMITTED);
  const freshDigests = digests(fresh);
  const names = [...new Set([...committedDigests.keys(), ...freshDigests.keys()])].sort();
  const differingFiles = names.filter((n) => committedDigests.get(n) !== freshDigests.get(n));

  const before = interrogate(COMMITTED);
  const after = interrogate(fresh);

  // The battery is this gate's scope, so an export it does not drive is a hole, not a pass.
  // Read the exports off the freshly built module rather than a list kept here, so adding
  // an export to the crate fails this check until the battery covers it.
  const exercised = new Set(battery().map((c) => c.fn));
  const freshKernel = require_(path.join(fresh, "sharpebench.js"));
  const uncovered = Object.keys(freshKernel)
    .filter((n) => typeof freshKernel[n] === "function" && !n.startsWith("__wbindgen"))
    .filter((n) => !exercised.has(n))
    .sort();

  const stampMismatch = [];
  if (before.methodologyVersion !== after.methodologyVersion) {
    stampMismatch.push(
      `methodology_version: committed ${before.methodologyVersion} vs built ${after.methodologyVersion}`,
    );
  }
  const missedGoldens = before.goldens.filter((g) => !g.ok).map((g) => g.name);
  const divergent = [...after.answers.keys()].filter(
    (label) => before.answers.get(label) !== after.answers.get(label),
  );

  if (stampMismatch.length === 0 && missedGoldens.length === 0 && divergent.length === 0 && uncovered.length === 0) {
    console.log(`ok   ${name} answers identically to this tree's wasm-pack build`);
    console.log(`     ${after.answers.size} calls across all ${exercised.size} exports, byte-identical returns`);
    console.log(`     methodology_version=${before.methodologyVersion}`);
    console.log(`     ${before.goldens.length} committed golden score file(s) reproduced by the committed bundle`);
    if (differingFiles.length === 0) {
      console.log(`     the two bundles are also byte-identical:`);
      for (const n of names) console.log(`       ${committedDigests.get(n)}  ${n}`);
    } else {
      console.log(`     note: the two bundles differ in bytes (${differingFiles.join(", ")}) while`);
      console.log(`     answering identically. wasm-pack output is not byte-reproducible across`);
      console.log(`     operating systems, and inside one host the binary embeds panic::Location`);
      console.log(`     records, so a docs-only edit above a panic site moves it. That is reported,`);
      console.log(`     not failed.`);
    }
    process.exit(0);
  }

  console.error("");
  if (uncovered.length > 0 && stampMismatch.length === 0 && missedGoldens.length === 0 && divergent.length === 0) {
    console.error(`FAIL the battery does not drive every export of the bundle this tree builds`);
    for (const fn of uncovered) console.error(`     uncovered export: ${fn}`);
    console.error("");
    console.error("     diagnosis: THE GATE IS INCOMPLETE, not the artifact. Two bundles agreeing");
    console.error("     on every input in the battery are taken as equivalent, so an export the");
    console.error("     battery never calls is unguarded. Add calls for the exports above to");
    console.error("     `battery()` in this file, including its refusal paths.");
    process.exit(1);
  }
  console.error(`FAIL ${name} does not answer like the bundle this tree builds`);
  for (const fn of uncovered) console.error(`     uncovered export (battery incomplete): ${fn}`);
  for (const line of stampMismatch) console.error(`     ${line}`);
  for (const golden of missedGoldens) {
    console.error(`     committed golden ${golden}: MISSED by the committed bundle`);
  }
  for (const label of divergent.slice(0, 12)) {
    const a = String(before.answers.get(label) ?? "<absent>");
    const b = String(after.answers.get(label) ?? "<absent>");
    console.error(`     ${label}:`);
    console.error(`       committed ${a.length > 160 ? `${a.slice(0, 160)}... (${a.length} chars)` : a}`);
    console.error(`       built     ${b.length > 160 ? `${b.slice(0, 160)}... (${b.length} chars)` : b}`);
  }
  if (divergent.length > 12) console.error(`     ... and ${divergent.length - 12} more divergent call(s)`);
  console.error("");
  if (stampMismatch.length > 0 || missedGoldens.length > 0) {
    console.error("     diagnosis: THE COMMITTED BUNDLE IS STALE. It reports a different methodology");
    console.error("     version than this source tree builds, or fails a committed golden, so it is");
    console.error("     the artifact that is wrong. Rebuild and commit it:");
    console.error("       wasm-pack build crates/sharpebench-wasm --target nodejs \\");
    console.error("         --out-dir ../../npm/pkg --out-name sharpebench");
  } else {
    console.error("     diagnosis: THE COMMITTED BUNDLE ANSWERS DIFFERENTLY while reporting the same");
    console.error("     methodology version and reproducing every committed golden. The version stamp");
    console.error("     moves only when the workspace version does, so a kernel change within one");
    console.error("     version lands here and nowhere else. Rebuild and commit the bundle; if the");
    console.error("     committed bundle is the intended one, this source tree is what moved.");
  }
  process.exit(1);
} finally {
  rmSync(workDir, { recursive: true, force: true, maxRetries: 5 });
}
