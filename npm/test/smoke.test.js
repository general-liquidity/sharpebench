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

test("greeks zero-volatility boundaries execute in the shipped wasm", () => {
  const raw = require("../pkg/sharpebench.js");
  const params = { spot: 100, strike: 100, t_years: 1, rate: 0.05, vol: 0, is_call: true };
  const result = sb.greeks(params);
  assert.ok(Math.abs(result.price - 4.877057549928594) < 1e-10);
  assert.equal(result.greeks.delta, 1);
  assert.equal(result.risk.net_short_gamma, false);
  assert.equal(Object.hasOwn(result.risk, "unbounded_tail"), false);
  assert.equal(Object.hasOwn(result.risk, "naked_short_gamma"), false);
  assert.deepEqual(JSON.parse(raw.greeks(JSON.stringify(params))), result);
  for (const invalid of [
    { ...params, vol: -0.1 },
    { ...params, spot: 0 },
    { ...params, t_years: -1 },
    { ...params, rate: 0 },
  ]) {
    const expected = invalid.rate === 0 ? /Greeks are undefined/ : /invalid options parameter/;
    assert.throws(() => sb.greeks(invalid), expected);
    // The raw export must refuse too; a JS-only guard would mask a stale wasm.
    assert.match(JSON.parse(raw.greeks(JSON.stringify(invalid))).error, expected);
  }
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

test("briefing audit aggregates repeated area identities in the shipped wasm", () => {
  const section = (asset_area) => ({
    asset_area,
    rows: [
      { text: "observable", kind: "fact" },
      { text: "uncertainty", kind: "uncertainty" },
    ],
  });
  const audit = sb.auditBriefing({
    sections: [section(" Energy "), section("ENERGY"), section("energy"), section("rates")],
  });
  assert.equal(audit.balanced, false);
  assert.deepEqual(audit.salience, [
    { asset_area: "energy", row_count: 6, salience: 0.75 },
    { asset_area: "rates", row_count: 2, salience: 0.25 },
  ]);
  assert.ok(audit.violations.some((v) => v.violation === "asset_area_overweight" && v.rows === 6));
  assert.equal(sb.auditBriefing({ sections: [section("energy")] }, { max_area_salience: 1 }).balanced, true);
  const unverified = sb.auditBriefing({
    sections: [section("energy"), section("rates")],
    return_table: { ordering: "unspecified", entries: [] },
  });
  assert.ok(unverified.violations.some((v) => v.violation === "unspecified_table_ordering"));
});

test("isMySharpeReal passes a long clean single-trial edge", () => {
  const returns = Array.from(
    { length: 400 },
    (_, i) => 0.001 + 0.00005 * ((i % 4) - 1.5),
  );
  const v = sb.isMySharpeReal(returns, { nTrials: 1 });
  assert.equal(v.verdict, "Pass");
  assert.equal(typeof v.haircutSharpe, "number");
  assert.equal(typeof v.deflatedSharpe, "number");
  assert.ok(v.haircut >= 0 && v.haircut <= 1, `haircut=${v.haircut}`);
});

test("isMySharpeReal fails a short series mined over many trials", () => {
  const returns = Array.from({ length: 30 }, (_, i) => 0.001 * ((i % 7) - 3));
  const v = sb.isMySharpeReal(returns, { nTrials: 1000 });
  assert.equal(v.verdict, "Fail");
});

test("honesty wrappers refuse invalid or overflowing search counts", () => {
  const returns = [0.01, 0.02, -0.01];
  for (const nTrials of [0, 2 ** 32, 2 ** 32 + 1, Number.MAX_SAFE_INTEGER,
                         -1, 1.5, NaN, Infinity, "10", true]) {
    assert.throws(() => sb.isMySharpeReal(returns, { nTrials }), /nTrials|n_trials/);
    assert.throws(() => sb.isMySharpeRealFull([returns], 0, { nTrials }), /nTrials|n_trials/);
  }
  assert.equal(sb.isMySharpeReal(returns, { nTrials: 2 ** 32 - 1 }).nTrials, 2 ** 32 - 1);
  // Bypass the TypeScript guard as well: a stale unsafe wasm must fail this test.
  const kernel = require("../pkg/sharpebench.js");
  const config = JSON.stringify({n_trials: 2 ** 32 + 1});
  for (const raw of [kernel.is_my_sharpe_real(JSON.stringify(returns), config),
                    kernel.is_my_sharpe_real_full(JSON.stringify([returns]), 0, config)]) {
    const result = JSON.parse(raw);
    assert.deepEqual(Object.keys(result), ["error"]);
    assert.match(result.error, /n_trials/);
  }
});

test("uncertainty refuses nonbinary outcomes inside the actual wasm module", () => {
  for (const value of [-1, 2, 0.3, NaN, Infinity, null, "true"]) {
    assert.throws(() => sb.decomposeUncertainty({outcomes: [0, value, 1]}), /outcomes\[1\]/);
  }
  assert.deepEqual(sb.decomposeUncertainty({outcomes: [1, 0, 1]}),
                   sb.decomposeUncertainty({outcomes: [true, false, true]}));
});

test("isMySharpeRealFull runs the multiple-testing family + PBO", () => {
  const field = Array.from({ length: 5 }, (_, j) =>
    Array.from(
      { length: 80 },
      (_, i) => (j === 2 ? 0.004 : 0.0005) + 0.003 * (((i + j) % 6) - 2.5),
    ),
  );
  const v = sb.isMySharpeRealFull(field, 2, { nTrials: 5 });
  assert.ok(["Pass", "Borderline", "Fail"].includes(v.honesty.verdict));
  assert.ok(v.pbo >= 0 && v.pbo <= 1, `pbo=${v.pbo}`);
  assert.ok(v.realityCheckP >= 0 && v.realityCheckP <= 1);
  assert.equal(v.stepDown.length, field.length);
});

test("regimeCompare reports a pooled sign reversal", () => {
  const report = sb.regimeCompare(
    [0.02, 0.03, -0.01, -0.02],
    [0.0, 0.01, 0.01, 0.02],
    ["calm", "calm", "stress", "stress"],
    { minPeriods: 2 },
  );
  assert.equal(report.regimes.length, 2);
  assert.equal(report.pooled_hides_reversal, true);
  assert.deepEqual(report.reversal_regimes, ["calm"]);
});

test("regimeCompare refuses misaligned arrays", () => {
  assert.throws(
    () => sb.regimeCompare([0.1], [0.1, 0.2], ["calm"]),
    /requires aligned arrays/,
  );
});
