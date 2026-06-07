FROM rust:1.86-bookworm AS builder

RUN rustup target add wasm32-unknown-unknown \
    && cargo install trunk --locked

WORKDIR /app
COPY . .

RUN cd crates/frontend && trunk build --release
RUN cargo build --release -p dreamwell-server

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /app/target/release/dreamwell-server /usr/local/bin/dreamwell-server
COPY --from=builder /app/crates/frontend/dist /app/static

ENV DREAMWELL_STATIC_DIR=/app/static \
    DREAMWELL_HOST=0.0.0.0 \
    DREAMWELL_PORT=8080 \
    DREAMWELL_DATABASE_URL=sqlite:/app/data/dreamwell.db

RUN mkdir -p /app/data
VOLUME ["/app/data"]
EXPOSE 8080

CMD ["dreamwell-server"]
