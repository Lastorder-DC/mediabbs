# syntax=docker/dockerfile:1

# ---- build ----
FROM rust:1-trixie AS build
RUN apt-get update \
    && apt-get install -y --no-install-recommends default-libmysqlclient-dev pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY Cargo.toml Cargo.lock build.rs ./
COPY migrations ./migrations
COPY src ./src
# Commit hash shown in the version ("0.3.0-1a2b3c4"); there is no .git in the
# image. Leave it out (or empty) for release images: plain "0.3.0".
#   docker build --build-arg MEDIABBS_GIT_HASH=$(git rev-parse --short=7 HEAD) .
ARG MEDIABBS_GIT_HASH=
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
