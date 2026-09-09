import test from "node:test";
import assert from "node:assert";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { InMemoryTransport } from "@modelcontextprotocol/sdk/inMemory.js";
import { createServer } from "../dist/server.js";

async function connectedClient() {
  const server = createServer();
  const [clientTransport, serverTransport] = InMemoryTransport.createLinkedPair();
  const client = new Client({ name: "test", version: "0.0.0" });
  await Promise.all([server.connect(serverTransport), client.connect(clientTransport)]);
  return client;
}

test("registers the kernel tools", async () => {
  const client = await connectedClient();
  const { tools } = await client.listTools();
  const names = tools.map((t) => t.name);
  for (const expected of [
    "score",
    "score_agent",
    "self_audit",
    "greeks",
    "canary",
    "is_my_sharpe_real",
    "regime_compare",
  ]) {
    assert.ok(names.includes(expected), `missing tool: ${expected}`);
  }
  await client.close();
});

test("is_my_sharpe_real tool renders a verdict", async () => {
  const client = await connectedClient();
  const returns = Array.from(
    { length: 400 },
    (_, i) => 0.001 + 0.00005 * ((i % 4) - 1.5),
  );
  const res = await client.callTool({
    name: "is_my_sharpe_real",
    arguments: { returns, n_trials: 1 },
  });
  const parsed = JSON.parse(res.content[0].text);
  assert.ok(["Pass", "Borderline", "Fail"].includes(parsed.verdict));
  assert.equal(typeof parsed.haircutSharpe, "number");
  await client.close();
});

test("honesty tool advertises and enforces the kernel search-count bounds", async () => {
  const client = await connectedClient();
  try {
    const { tools } = await client.listTools();
    const schema = tools.find((tool) => tool.name === "is_my_sharpe_real").inputSchema;
    assert.equal(schema.properties.n_trials.type, "integer");
    assert.equal(schema.properties.n_trials.minimum, 1);
    assert.equal(schema.properties.n_trials.maximum, 2 ** 32 - 1);
    for (const n_trials of [0, 2 ** 32, 2 ** 32 + 1, 1.5]) {
      const result = await client.callTool({name: "is_my_sharpe_real",
        arguments: {returns: [0.01, 0.02, -0.01], n_trials}});
      assert.equal(result.isError, true);
      assert.match(result.content[0].text, /n_trials/);
    }
  } finally {
    await client.close();
  }
});

test("greeks tool prices an ATM call to ~10.4506", async () => {
  const client = await connectedClient();
  const res = await client.callTool({
    name: "greeks",
    arguments: { spot: 100, strike: 100, t_years: 1, rate: 0.05, vol: 0.2, is_call: true },
  });
  const parsed = JSON.parse(res.content[0].text);
  assert.ok(Math.abs(parsed.price - 10.4506) < 1e-2, `price=${parsed.price}`);
  await client.close();
});

test("greeks tool preserves zero-volatility pricing and explicit refusals", async () => {
  const client = await connectedClient();
  try {
    const params = { spot: 100, strike: 100, t_years: 1, rate: 0.05, vol: 0, is_call: true };
    const response = await client.callTool({ name: "greeks", arguments: params });
    assert.notEqual(response.isError, true);
    const quote = JSON.parse(response.content[0].text);
    assert.ok(Math.abs(quote.price - 4.877057549928594) < 1e-10);
    assert.equal(quote.risk.net_short_gamma, false);
    assert.equal(Object.hasOwn(quote.risk, "unbounded_tail"), false);
    for (const [change, expected] of [
      [{ vol: -0.1 }, /invalid options parameter: vol/],
      [{ rate: 0 }, /Greeks are undefined/],
    ]) {
      const result = await client.callTool({ name: "greeks", arguments: { ...params, ...change } });
      assert.equal(result.isError, true);
      assert.match(result.content[0].text, expected);
    }
  } finally {
    await client.close();
  }
});

test("self_audit tool reports all defended", async () => {
  const client = await connectedClient();
  const res = await client.callTool({ name: "self_audit", arguments: {} });
  const parsed = JSON.parse(res.content[0].text);
  assert.equal(parsed.all_defended, true);
  await client.close();
});

test("regime_compare tool executes the WASM kernel", async () => {
  const client = await connectedClient();
  const res = await client.callTool({
    name: "regime_compare",
    arguments: {
      returns_a: [0.02, 0.03, -0.01, -0.02],
      returns_b: [0.0, 0.01, 0.01, 0.02],
      regimes: ["calm", "calm", "stress", "stress"],
      min_periods: 2,
    },
  });
  assert.notEqual(res.isError, true);
  const parsed = JSON.parse(res.content[0].text);
  assert.equal(parsed.pooled_hides_reversal, true);
  assert.deepEqual(parsed.reversal_regimes, ["calm"]);
  const calm = parsed.regimes.find((r) => r.regime === "calm");
  assert.equal(typeof calm.near_zero_return_mass_gap, "number");
  assert.equal(calm.b.near_zero_return_mass, 0.5);
  assert.equal(Object.hasOwn(calm, "zero_mass_gap"), false, "pre-rename key must not reach the wire");
  await client.close();
});
