const test = require("node:test");
const assert = require("node:assert");
const sb = require("../dist/index.js");

// A JS string reaches the kernel as UTF-8. A lone UTF-16 surrogate has no UTF-8
// encoding: `TextEncoder` in the wasm-bindgen glue would hand the kernel U+FFFD in
// its place and the call would succeed on text the caller never wrote. The wrapper
// refuses it by name instead, on every entry point that takes text.

const HIGH = "\uD800";
const LOW = "\uDC00";

const returns = [0.002, 0.0021, 0.0019, 0.002, 0.0022];
const submission = (agentId) => ({ agent_id: agentId, runs: [{ returns }] });
const briefing = (area) => ({
  sections: [{ asset_area: area, rows: [{ text: "rates", kind: "fact" }] }],
});

function assertRefused(f, where) {
  assert.throws(f, (e) => {
    assert.ok(e instanceof sb.InvalidUtf8Error, `expected InvalidUtf8Error, got ${e.name}: ${e.message}`);
    assert.match(e.message, /lone UTF-16 surrogate/);
    assert.ok(e.message.includes(where), `message names ${where}: ${e.message}`);
    return true;
  });
}

test("lone and misordered surrogates in a string value are refused by name", () => {
  for (const bad of ["a" + HIGH, LOW + "a", "a" + LOW + HIGH]) {
    assertRefused(() => sb.scoreAgent(submission(bad)), "submission.agent_id");
  }
});

test("a lone surrogate passed straight to the glue is refused, not encoded as U+FFFD", () => {
  // canary takes its seed as a bare string, the path TextEncoder would rewrite.
  assertRefused(() => sb.canary("seed" + HIGH), "seed");
});

test("a lone surrogate in an object key is refused", () => {
  assertRefused(
    () => sb.scoreAllocation({ steps: [{ weights: [1.0] }], ["x" + HIGH]: 1 }),
    "trajectory key",
  );
});

test("every entry point that takes text checks it", () => {
  assertRefused(() => sb.score([submission(HIGH)]), "submissions.agent_id");
  assertRefused(() => sb.classifyDisqualification([submission(HIGH)]), "submissions.agent_id");
  assertRefused(() => sb.auditBriefing(briefing("fx" + HIGH)), "briefing.asset_area");
  assertRefused(() => sb.auditBriefing({ sections: [] }, { [HIGH]: 1 }), "policy key");
  assertRefused(
    () => sb.regimeCompare([0.1, 0.2], [0.1, 0.2], ["calm", "stress" + HIGH]),
    "regimes.1",
  );
  assertRefused(() => sb.percentileSelection([returns], { utility: HIGH }), "opts.utility");
  assertRefused(
    () => sb.greeks({ spot: 100, strike: 100, t_years: 1, rate: 0.05, vol: 0.2, is_call: true, [LOW]: 0 }),
    "params key",
  );
});

test("well-formed text, including a real pair and a literal U+FFFD, still runs", () => {
  for (const text of ["AAA", "A\u{1F600}A", "A�A"]) {
    assert.equal(sb.scoreAgent(submission(text)).agent_id, text, text);
    assert.equal(sb.canary(text).token.length, 64, text);
    const report = sb.regimeCompare([0.1, 0.2], [0.1, 0.2], [text, text]);
    assert.equal(report.regimes[0].regime, text, text);
  }
  // A literal U+FFFD is text the caller wrote, so it keeps its own canary.
  assert.notDeepEqual(sb.canary("A�A"), sb.canary("AAA"));
});
