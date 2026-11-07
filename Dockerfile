# syntax=docker/dockerfile:1

# ---- build ----
FROM rust:1-trixie AS build
RUN apt-get update \
    && apt-get install -y --no-install-recommends default-libmysqlclient-dev pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY migrations ./migrations
COPY src ./src
RUN cargo build --release --locked

# ---- runtime ----
FROM debian:trixie-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends libmariadb3 libssl3t64 ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --home-dir /app mediabbs \
    && mkdir -p /app/uploads \
    && chown mediabbs /app/uploads
COPY --from=build /src/target/release/mediabbs /usr/local/bin/mediabbs
WORKDIR /app
USER mediabbs
ENV BIND_ADDRESS=0.0.0.0:8080 \
    OAUTH_PUBLIC_KEY_PATH=/app/pubkey.pem \
    LOCAL_UPLOAD_DIR=/app/uploads
EXPOSE 8080
VOLUME ["/app/uploads"]
ENTRYPOINT ["mediabbs"]
CMD ["serve"]
