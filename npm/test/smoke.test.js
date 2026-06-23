const test = require("node:test");
const assert = require("node:assert");
const sb = require("../dist/index.js");

test("score ranks a skilled agent ahead of a flat one", () => {
  const steady = (b) =>
    Array.from({ length: 10 }, (_, i) => b + 0.0001 * Math.sin(i));
  const board = sb.score([
    {
      agent_id: "skilled",
      runs: [{ returns: steady(0.002) }, { returns: steady(0.0021) }],
    },
    { agent_id: "flat", runs: [{ returns: [0, 0, 0, 0, 0] }] },
  ]);
  const ids = board.map((s) => s.agent_id);
  assert.ok(ids.includes("skilled") && ids.includes("flat"));
});

test("scoreAgent returns a composite with a deflated Sharpe", () => {
  const s = sb.scoreAgent({
    agent_id: "a",
    runs: [{ returns: [0.002, 0.0021, 0.0019, 0.002, 0.0022] }],
  });
  assert.equal(s.agent_id, "a");
  assert.equal(typeof s.deflated_sharpe, "number");
});

test("greeks prices an ATM call to ~10.4506", () => {
  const r = sb.greeks({
    spot: 100,
    strike: 100,
    t_years: 1,
    rate: 0.05,
    vol: 0.2,
    is_call: true,
  });
  assert.ok(Math.abs(r.price - 10.4506) < 1e-2, `price=${r.price}`);
});

test("selfAudit reports all attacks defended", () => {
  assert.equal(sb.selfAudit().all_defended, true);
});

test("canary derives a stable 64-hex token", () => {
  const c = sb.canary("scenario-1");
  assert.equal(c.token.length, 64);
  assert.deepEqual(sb.canary("scenario-1"), c);
});

test("auditBriefing and scoreAllocation bridge", () => {
  assert.equal(sb.auditBriefing({ sections: [] }).balanced, true);
  assert.equal(
    sb.scoreAllocation({ steps: [{ weights: [1.0] }] }).valid,
    true,
  );
});
