# syntax=docker/dockerfile:1

FROM lukemathwalker/cargo-chef:latest-rust-1.87 AS chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN apt-get update \
 && apt-get install -y --no-install-recommends build-essential cmake libssl-dev pkg-config \
 && rm -rf /var/lib/apt/lists/*
# Cache-friendly dependency build (RandomX C core included)
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN cargo build --release --locked --bin slash-node

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl \
 && rm -rf /var/lib/apt/lists/* \
 && groupadd -r slash \
 && useradd -r -g slash -d /home/slash slash
WORKDIR /home/slash
COPY --from=builder /app/target/release/slash-node /usr/local/bin/slash-node
USER slash
EXPOSE 30333/tcp 9944/tcp
VOLUME ["/home/slash/.slash"]
HEALTHCHECK --interval=30s --timeout=5s --start-period=30s --retries=3 \
  CMD curl -fsS -X POST -H "Content-Type: application/json" \
      -d '{"jsonrpc":"2.0","method":"getTip","params":[],"id":1}' \
      http://localhost:9944/ || exit 1
ENTRYPOINT ["/usr/local/bin/slash-node"]
