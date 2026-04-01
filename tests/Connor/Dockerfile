FROM rust:1.86-bookworm AS builder

WORKDIR /app

COPY Cargo.toml Cargo.lock* ./
COPY src ./src
COPY public ./public
COPY style ./style

RUN cargo build --release --bin specifications --no-default-features --features ssr

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/specifications /app/specifications

COPY .env.fly .env
ENV PORT=8080
ENV RUST_LOG=info

EXPOSE 8080

CMD ["/app/specifications"]
