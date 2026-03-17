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

RUN cargo build --release

FROM debian:bookworm-slim AS runtime
WORKDIR /work

# Runtime requirements for HTTPS + CLI behavior
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Install reen binary
COPY --from=builder /app/target/release/reen /usr/local/bin/reen

# Run the CLI directly; pass subcommands as docker args.
ENTRYPOINT ["reen"]
CMD ["--help"]
