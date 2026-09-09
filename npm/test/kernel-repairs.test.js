const test = require("node:test");
const assertKernelRepairContract = require("./kernel-repair-contract.cjs");
const sb = require("../dist/index.js");
const { version } = require("../package.json");

test("rebuilt wasm and the wrapper preserve statistical refusals", () => {
  assertKernelRepairContract(sb, version);
});
