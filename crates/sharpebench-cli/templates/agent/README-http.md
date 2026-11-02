# {{NAME}}

A SharpeBench agent that serves the HTTP protocol: `POST /decide` with a
MarketObservation body, answered with a Decision body. `sharpebench init-agent`
wrote this directory.

## Files

- `server.py` holds the policy and the server. Replace `decide` with your own.
- `sharpebench-agent.json` records the address `sharpebench check-agent` sends
  requests to.

## Check it

The harness neither starts nor stops an HTTP endpoint, so start it first:

```sh
{{PYTHON}} {{DIR}}/server.py --port 8765
```

Then, in another shell, run the checks against a small synthetic window:

```sh
sharpebench check-agent {{DIR}}
```

The report lists every gate the agent would fail and why. The book page
"Submit your agent" explains each gate. If you start the server on another
port, pass the address directly: `sharpebench check-agent --http 127.0.0.1:<port>`.

## Run it

```sh
sharpebench run --http 127.0.0.1:8765
```

The endpoint keeps whatever it holds in memory across every run, and it can
reach the network, so the row it produces on a historical window is not
certifying. To be scored under the hardened container, package the policy as an
image (`sharpebench init-agent <dir> --kind image`) and run it with `--image`.
