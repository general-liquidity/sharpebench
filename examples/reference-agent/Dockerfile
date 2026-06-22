# Reference SharpeBench agent — stdio transport, container contract.
#
#   docker build -t sb-reference-agent .
#   docker run -i --rm sb-reference-agent      # then feed it MarketObservation JSON lines
#
# The harness runs the container and speaks newline-delimited JSON over stdio:
#   harness -> stdin : one MarketObservation (JSON) per line
#   agent   -> stdout: one Decision (JSON) per line
#
# `-u` keeps Python's stdout unbuffered so each Decision flushes immediately —
# required for the line-synchronous request/response loop.
FROM python:3.12-slim
WORKDIR /app
COPY agent.py /app/agent.py
ENTRYPOINT ["python3", "-u", "/app/agent.py"]
