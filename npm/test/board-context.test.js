const test = require("node:test");
const assert = require("node:assert/strict");
const sb = require("../dist/index.js");

function field() {
  const track = (mean) => Array.from({ length: 60 }, (_, i) => mean + 0.001 * Math.sin(i * 0.7));
  return [
    { agent_id: "candidate", runs: [{ returns: track(0.01) }], declared_mandate: { kind: "relative_to", benchmark_id: "reference" } },
    { agent_id: "reference", runs: [{ returns: track(0.02) }] },
  ];
}

test("declared mandates survive the shipped wasm board boundary", () => {
  const board = sb.score(field());
  const candidate = board.find((s) => s.agent_id === "candidate");
  assert.equal(candidate.passed_k, true);
  assert.equal(candidate.declared_passed_k, false);
  assert.equal(candidate.declared_mandate_eligible, false);
  assert.deepEqual(candidate.declared_mandate, { kind: "relative_to", benchmark_id: "reference" });
  const undeclared = sb.score(field().map(({ declared_mandate, ...rest }) => rest));
  const host = (rows) => rows.map((s) => [s.agent_id, s.rank_eligible, s.rank_ordinal, s.deflated_sharpe]);
  assert.deepEqual(host(board), host(undeclared));
});

test("disqualification explains the field-relative board through actual wasm", () => {
  const config = {
    n_trials: 2, trials_sr_std: 0.5, dsr_bar: 0.95, per_run_psr_bar: 0.9,
    alpha: 0.05, bootstrap_seed: 7, n_boot: 99, block_prob: 0.1,
    pass_mode: "relative_to_benchmark", benchmark_agent_id: "candidate",
  };
  const board = sb.score(field(), config);
  const reasons = sb.classifyDisqualification(field(), config);
  assert.equal(reasons.length, board.length);
  for (const row of board) {
    const passes = row.agent_id === "reference";
    const explanation = reasons.find((s) => s.agent_id === row.agent_id);
    assert.equal(row.passed_k, passes);
    assert.equal(explanation.rank_eligible, row.rank_eligible);
    assert.equal(explanation.reasons.includes("failed_pass_k"), !passes);
  }
  assert.deepEqual(reasons.map((s) => s.agent_id), board.map((s) => s.agent_id));
  // Fieldless scoring cannot resolve a relative benchmark. The old classifier
  // therefore failed even the stronger agent that beats it in this field.
  assert.ok(field().every((s) => !sb.scoreAgent(s, config).passed_k));
});

test("both board endpoints refuse invalid declarations and ambiguous identities", () => {
  for (const input of [
    [{ agent_id: "a", runs: [] }, { agent_id: "a", runs: [] }],
    [{ agent_id: "  ", runs: [] }],
    [{ agent_id: "a", runs: [], declared_mandate: { kind: "relative_typo" } }],
  ]) {
    assert.throws(() => sb.score(input));
    assert.throws(() => sb.classifyDisqualification(input));
  }
});
