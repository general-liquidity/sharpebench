"""Historical synthetic reproductions for the 2026-09-07 audit baselines.

Requires Linux and sibling sharpebench/sharpearena baseline checkouts in the
workspace supplied as the sole argument. Build the reviewed Bench CLI and create
the unpublished Arena 0.24.1 Cargo archive first; see README.md for exact commits.
The SPEC_FILES/SPEC_EPOCH parser intentionally describes the historical build.rs.
This is archived reproduction evidence, not a regression suite for current code.

No model, network, broker, container, or product-file writes by this script.
Linux memfd carries the synthetic JSON into the CLI.
"""
import copy
import hashlib
import json
import math
import os
from pathlib import Path
import re
import subprocess
import sys
import tarfile

if len(sys.argv) != 2:
    raise SystemExit('Usage: python3 reproduce_core_findings.py /path/to/baseline-workspace')
ROOT = Path(sys.argv[1]).expanduser().resolve()
BENCH = ROOT / 'sharpebench'
ARENA = ROOT / 'sharpearena'
CLI = BENCH / 'target/debug/sharpebench'

def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=False, allow_nan=False).encode()

def invoke(command, docs, extra=()):
    descriptors = []
    try:
        for value in docs:
            fd = os.memfd_create('sharpe-synthetic-audit')
            os.write(fd, canonical(value))
            os.lseek(fd, 0, os.SEEK_SET)
            descriptors.append(fd)
        result = subprocess.run([str(CLI), command, *[f'/proc/self/fd/{fd}' for fd in descriptors], '--json', *extra], pass_fds=descriptors, capture_output=True, text=True, timeout=60)
        return result.returncode, json.loads(result.stdout) if result.returncode == 0 else result.stderr.strip()
    finally:
        for fd in descriptors:
            os.close(fd)

def report(label, result):
    print(json.dumps({'probe': label, 'observed': result}, sort_keys=True))

run = {'returns': [0.003 + 0.001 * math.sin(i) for i in range(200)], 'trace': {'events': []}}
bad = copy.deepcopy(run)
bad['trace']['events'].append({'event': 'denylist_bypass'})
victim = {'agent_id': 'victim', 'runs': [run, bad]}
for peers in ([], [{'agent_id': 'short', 'runs': [run]}]):
    status, result = invoke('score', [[victim, *peers]])
    if status == 0:
        # The public command emits a bare array of CompositeScore rows.
        row = next(x for x in result if x['agent_id'] == 'victim')
        result = {k: row[k] for k in ('rank_eligible', 'process_ok', 'runs_submitted', 'runs_scored')}
    report('process_with_short_peer' if peers else 'process_without_peer', [status, result])

status, result = invoke('score', [[{'agent_id': 'paired-executions', 'runs': [{'returns': [.1, -.2]}, {'returns': [-.1, .2]}]}]], ('--execution-seeds-per-window', '2'))
report('per_run_drawdown_can_exceed_pooled', [status, {k: result[0][k] for k in ('max_drawdown', 'worst_run_drawdown')} if status == 0 else result])

fixture = json.loads((BENCH / 'examples/forecast-quality/fixtures/agent-alpha.json').read_text())
for key in ('contracts', 'revisions', 'resolutions'):
    fixture[key] = fixture[key][:1]

def evidence(name, predictions, contract_changes=None, outcomes=None):
    doc = copy.deepcopy(fixture)
    doc['identity']['agent_id'] = name
    doc['contracts'] = []
    doc['revisions'] = []
    doc['resolutions'] = []
    for i, prediction in enumerate(predictions):
        c = copy.deepcopy(fixture['contracts'][0])
        c['contract_id'] = f'audit-{i}'
        if contract_changes:
            c.update(contract_changes[i])
        r = copy.deepcopy(fixture['revisions'][0])
        r.update(claim_id=f'claim-{i}', revision_id=f'claim-{i}:r0', idempotency_key=f'{name}:{i}', prediction=[prediction], contract_sha256=hashlib.sha256(canonical(c)).hexdigest())
        settlement = copy.deepcopy(fixture['resolutions'][0])
        settlement.update(claim_id=f'claim-{i}', outcome=outcomes[i] if outcomes else 1.0)
        doc['contracts'].append(c)
        doc['revisions'].append(r)
        doc['resolutions'].append(settlement)
    return doc

a = evidence('a', [.9]); b = evidence('b', [.6])
status, result = invoke('forecast-quality', [a, b])
report('one_settlement_block', [status, result['comparisons'] if status == 0 else result])
b['resolutions'][0]['outcome'] = 0.0
status, result = invoke('forecast-quality', [a, b])
report('contradictory_same_contract_outcomes', [status, result['comparisons'] if status == 0 else result])

a = evidence('a', [.9], [{'neutral_threshold': 1e-5}])
report('python_canonical_float_digest', invoke('forecast-quality', [a]))

# Identical predictions expressed in different monetary units must not reverse
# a dimensionless composite conclusion unless a weighting rule declares it.
for scale, unit in ((1.0, 'USD'), (100.0, 'US_cents')):
    changes = [{}, {'kind': 'point', 'scoring_rule': 'point_errors', 'unit': unit, 'target': 'price'}]
    a = evidence('a', [.9, .2 * scale], changes, [1.0, 0.0])
    b = evidence('b', [.1, .1 * scale], changes, [1.0, 0.0])
    status, result = invoke('forecast-quality', [a, b])
    report(f'mixed_units_{unit}', [status, result['comparisons'] if status == 0 else result])

build = (ARENA / 'crates/sharpearena/build.rs').read_text()
files = re.findall(r'"([^"]+)"', re.search(r'const SPEC_FILES.*?= \[(.*?)\];', build, re.S).group(1))
epoch = re.search(r'const SPEC_EPOCH:.*?= b"([^"]+)"', build).group(1).encode()

def spec_hash(read):
    h = 0xcbf29ce484222325
    chunks = [epoch]
    for name in files:
        value = read(name).replace(b'\r\n', b'\n')
        chunks.extend([name.encode() + b'\0', len(value).to_bytes(8, 'little'), value])
    for chunk in chunks:
        for byte in chunk:
            h = ((h ^ byte) * 0x100000001b3) & ((1 << 64) - 1)
    return f'{h:016x}'

archive = ARENA / 'target/package/sharpearena-0.24.1.crate'
with tarfile.open(archive) as tf:
    prefix = 'sharpearena-0.24.1/'
    report('source_vs_cargo_packaged_SPEC_HASH', {
        'source': spec_hash(lambda name: (ARENA / 'crates/sharpearena' / name).read_bytes()),
        'packaged': spec_hash(lambda name: tf.extractfile(prefix + name).read()),
        'packaged_with_original_manifest': spec_hash(lambda name: tf.extractfile(prefix + ('Cargo.toml.orig' if name == 'Cargo.toml' else name)).read()),
    })
