// The same assertions run against the working-tree wrapper and an offline
// installed tarball. Only the supplied public API is used to compute results.
const assert = require("node:assert/strict");

module.exports = function assertKernelRepairContract(sb, version) {
  const returns = Array.from({ length: 60 }, (_, i) => 0.002 + 0.0005 * ((i % 3) - 1));
  const field = [{ agent_id: "strong", runs: [{ returns }] }];
  const config = {
    n_trials: 2, trials_sr_std: 0.5, dsr_bar: 0.95, per_run_psr_bar: 0.9,
    alpha: 0.05, bootstrap_seed: 7, n_boot: 99, block_prob: 0.1,
    dsr_ci_level: 1.5,
  };
  const [diagnostic] = sb.classifyDisqualification(field, config);
  assert.equal(diagnostic.rank_eligible, false);
  assert.ok(diagnostic.reasons.includes("deflation_unavailable"));

  const selection = sb.percentileSelection([[], [-0.02, -0.01, -0.03]], { nBoot: 10 });
  assert.equal(selection.selected, null);
  assert.equal(selection.point_argmax, null);
  assert.equal(typeof selection.input_error, "string");

  const lite = sb.isMySharpeReal(returns, { nTrials: 2, trialsSrStd: -1 });
  assert.equal(lite.verdict, "Fail");
  assert.equal(typeof lite.statisticsError, "string");
  assert.equal(lite.methodologyVersion, `sharpebench-stats/${version}`);

  const badFrequency = sb.isMySharpeReal(returns, { nTrials: 2, periodsPerYear: 0 });
  assert.equal(badFrequency.verdict, "Fail");
  assert.equal(badFrequency.statisticsError, "periods_per_year must be finite and positive");
  const weekly = sb.isMySharpeReal(returns, { nTrials: 2, periodsPerYear: 52 });
  const daily = sb.isMySharpeReal(returns, { nTrials: 2, periodsPerYear: 252 });
  assert.ok(weekly.expectedMaxSharpe > daily.expectedMaxSharpe);

  const overflow = sb.isMySharpeReal([Number.MAX_VALUE, Number.MAX_VALUE, Number.MAX_VALUE], { nTrials: 2 });
  assert.equal(overflow.verdict, "Fail");
  assert.equal(typeof overflow.statisticsError, "string");

  const full = sb.isMySharpeRealFull([[1e308, 1e308], [1e308, 1e308]], 0, { nTrials: 2 });
  assert.deepEqual(full.stepDown, [false, false]);
  assert.deepEqual([full.realityCheckP, full.spaP, full.spaConsistentP], [1, 1, 1]);
  assert.equal(typeof full.snoopingError, "string");
  assert.equal(typeof full.pboError, "string");
  assert.equal(full.pbo, null);
  assert.equal(full.hlz.passed, false);
  assert.equal(full.hlz.tStat, null);

  const ordinary = sb.isMySharpeRealFull([returns, returns.map((x) => x - 0.0001)], 0, { nTrials: 2 });
  assert.equal(ordinary.snoopingError, undefined);
  assert.equal(ordinary.honesty.statisticsError, undefined);
  assert.equal(ordinary.hlz.tThreshold, 3);
  assert.equal(typeof ordinary.hlz.explanation, "string");
};
