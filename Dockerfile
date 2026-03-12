# syntax=docker/dockerfile:1

FROM rust:slim-bookworm AS builder
WORKDIR /app

# Build dependencies
RUN apt-get update \
    && apt-get install -y --no-install-recommends pkg-config libssl-dev ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Cache dependency build separately
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY agents ./agents
COPY runner.py ./runner.py

RUN cargo build --release

FROM python:3.12-slim-bookworm AS runtime
WORKDIR /work

# Runtime requirements for HTTPS + CLI behavior
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Python deps used by embedded runner.py
COPY requirements.txt /tmp/requirements.txt
RUN pip install --no-cache-dir -r /tmp/requirements.txt \
    && rm -f /tmp/requirements.txt

# Install reen binary
COPY --from=builder /app/target/release/reen /usr/local/bin/reen

# Run the CLI directly; pass subcommands as docker args.
ENTRYPOINT ["reen"]
CMD ["--help"]
