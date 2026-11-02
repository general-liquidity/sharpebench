# A minimal SharpeBench agent image. The harness runs it with no network, a
# read-only root filesystem, user 65532, a memory limit and one CPU, and speaks
# the stdio protocol with the entrypoint: one MarketObservation JSON object per
# line on stdin, one Decision JSON object per line on stdout.
#
# For a reproducible build, replace the tag below with the base image's
# digest (FROM python:3.12-slim@sha256:...).
FROM python:3.12-slim

# The root filesystem is read-only at run time, so the interpreter must not try
# to write bytecode, and output must not sit in a buffer.
ENV PYTHONDONTWRITEBYTECODE=1 \
    PYTHONUNBUFFERED=1

WORKDIR /agent
COPY agent.py /agent/agent.py

ENTRYPOINT ["python", "/agent/agent.py"]
