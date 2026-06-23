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
  for (const expected of ["score", "score_agent", "self_audit", "greeks", "canary"]) {
    assert.ok(names.includes(expected), `missing tool: ${expected}`);
  }
  await client.close();
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

test("self_audit tool reports all defended", async () => {
  const client = await connectedClient();
  const res = await client.callTool({ name: "self_audit", arguments: {} });
  const parsed = JSON.parse(res.content[0].text);
  assert.equal(parsed.all_defended, true);
  await client.close();
});
