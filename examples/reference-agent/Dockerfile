# Reference SharpeBench agent (Rust) — stdio transport, container contract.
#
# Build from the REPO ROOT (the agent uses the workspace's sharpebench-protocol):
#   docker build -f examples/reference-agent/Dockerfile -t sharpebench-reference-agent .
#   docker run -i --rm sharpebench-reference-agent      # then feed it MarketObservation JSON lines
#
# The harness runs the container and speaks newline-delimited JSON over stdio:
#   harness -> stdin : one MarketObservation (JSON) per line
#   agent   -> stdout: one Decision (JSON) per line
FROM rust:1.96-slim AS build
WORKDIR /src
COPY . .
RUN cargo build --release -p reference-agent

FROM debian:bookworm-slim
COPY --from=build /src/target/release/reference-agent /usr/local/bin/reference-agent
ENTRYPOINT ["reference-agent"]
