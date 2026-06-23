# syntax=docker/dockerfile:1

ARG RUST_VERSION=1.93
ARG TRUNK_VERSION=v0.21.14
ARG TRUNK_BASE_URL=https://github.com/trunk-rs/trunk/releases/download

FROM rust:${RUST_VERSION}-bookworm AS rust-base
ARG TRUNK_VERSION
ARG TRUNK_BASE_URL
RUN rustup component add rustfmt clippy \
    && rustup target add wasm32-unknown-unknown \
    && curl -fsSL "${TRUNK_BASE_URL}/${TRUNK_VERSION}/trunk-x86_64-unknown-linux-gnu.tar.gz" \
    | tar -xz -C /usr/local/cargo/bin trunk

FROM rust-base AS dev-base
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    cargo install cargo-watch --locked \
    && git config --global --add safe.directory /app
WORKDIR /app

FROM rust-base AS builder
WORKDIR /app
COPY . .

ARG GIT_SHA=dev
ENV DREAMWELL_GIT_SHA=${GIT_SHA}

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/app/target \
    cd crates/frontend && trunk build --release \
    && cp -r ../../.frontend-dist /app/frontend-dist

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/app/target \
    cargo build --release -p dreamwell-server \
    && cp /app/target/release/dreamwell-server /app/dreamwell-server

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /app/dreamwell-server /usr/local/bin/dreamwell-server
COPY --from=builder /app/frontend-dist /app/static

ENV DREAMWELL_STATIC_DIR=/app/static \
    DREAMWELL_HOST=0.0.0.0 \
    DREAMWELL_PORT=8080 \
    DREAMWELL_DATABASE_URL=sqlite:/app/data/dreamwell.db

RUN mkdir -p /app/data
VOLUME ["/app/data"]
EXPOSE 8080

CMD ["dreamwell-server"]
