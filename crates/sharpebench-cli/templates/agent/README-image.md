# {{NAME}}

A SharpeBench agent packaged as a container image. The harness starts a fresh
container for every run, with no network, and speaks the stdio protocol with
its entrypoint. `sharpebench init-agent` wrote this directory.

## Files

- `agent.py` holds the policy. Replace `decide` with your own.
- `Dockerfile` packages `agent.py` on a Python base image.
- `sharpebench-agent.json` records the image reference `sharpebench
  check-agent` launches. Replace it with the pinned reference once you have one.

## Build and pin it

```sh
docker build -t {{IMAGE}} {{DIR}}
docker image inspect --format '{{index .RepoDigests 0}}' {{IMAGE}}
```

The harness runs only a digest-pinned reference (`repository@sha256:...`) that
is already present on the host, and it never pulls. An image gets a repository
digest when you push it to a registry, or at build time on Docker's containerd
image store. If the second command fails or prints nothing, push the image to a
registry you control and pull it back by digest.

## Check it

```sh
sharpebench check-agent --image <repository@sha256:...>
```

The report lists every gate the agent would fail and why. The book page
"Submit your agent" explains each gate. Before you build, you can run the same
policy as a host process to iterate faster:

```sh
sharpebench check-agent --cmd "{{PYTHON}} {{DIR}}/agent.py"
```

## Run it

```sh
sharpebench run --image <repository@sha256:...>
```
