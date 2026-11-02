# {{NAME}}

A SharpeBench agent that runs as a host process and speaks the stdio protocol.
`sharpebench init-agent` wrote this directory.

## Files

- `agent.py` holds the policy. Replace `decide` with your own.
- `sharpebench-agent.json` tells `sharpebench check-agent` how to start the
  agent. Its `cmd` is the command line, and a word of it that names a file in
  this directory is resolved against this directory.

## Check it

Run the checks against a small synthetic window from the directory that holds
this one:

```sh
sharpebench check-agent {{DIR}}
```

The report lists every gate the agent would fail and why. The book page
"Submit your agent" explains each gate.

You can also talk to the agent by hand. It reads one observation per line and
answers with one decision per line:

```sh
echo '{"date":"2026-01-02","cash":1.0,"symbols":[{"symbol":"AAA","close_history":[1.0,1.1]}],"portfolio":[]}' | {{PYTHON}} {{DIR}}/agent.py
```

## Run it

```sh
sharpebench run --cmd "{{PYTHON}} {{DIR}}/agent.py"
```

`--cmd` runs the agent on this host with no sandbox, and the agent can reach
the network, so the row it produces on a historical window is not certifying.
To be scored under the hardened container, package the same `agent.py` as an
image (`sharpebench init-agent <dir> --kind image`) and run it with `--image`.
