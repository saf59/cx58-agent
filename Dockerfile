# syntax=docker/dockerfile:1

ARG RUST_VERSION=1.99
ARG APP_NAME=cx58-agent
ARG SCCACHE_VERSION=0.17.0

################################################################################
FROM rust:${RUST_VERSION}-alpine AS build
ARG APP_NAME
ARG SCCACHE_VERSION
WORKDIR /app

# openssl-libs-static — критически важен для устранения ошибки "cannot find -lssl"
RUN apk add --no-cache \
    build-base \
    ca-certificates \
    pkgconfig \
    openssl-dev \
    openssl-libs-static \
    git \
    wget

# Переменные окружения для статической линковки OpenSSL
ENV OPENSSL_STATIC=1
ENV OPENSSL_NO_VENDOR=1
# для отключения sqlx проверок при сборке
ENV SQLX_OFFLINE=true
ENV RUSTC_WRAPPER=/usr/local/bin/sccache
ENV SCCACHE_DIR=/var/cache/sccache

RUN wget -qO /tmp/sccache.tar.gz \
      "https://github.com/mozilla/sccache/releases/download/v${SCCACHE_VERSION}/sccache-v${SCCACHE_VERSION}-x86_64-unknown-linux-musl.tar.gz" \
    && tar -xzf /tmp/sccache.tar.gz -C /tmp \
    && mv "/tmp/sccache-v${SCCACHE_VERSION}-x86_64-unknown-linux-musl/sccache" /usr/local/bin/sccache \
    && chmod +x /usr/local/bin/sccache \
    && rm -rf /tmp/sccache.tar.gz "/tmp/sccache-v${SCCACHE_VERSION}-x86_64-unknown-linux-musl"

RUN --mount=type=bind,source=src,target=src \
    --mount=type=bind,source=Cargo.toml,target=Cargo.toml \
    --mount=type=bind,source=Cargo.lock,target=Cargo.lock \
    --mount=type=bind,source=.sqlx,target=.sqlx \
    --mount=type=bind,source=migrations,target=migrations \
    --mount=type=bind,source=locales,target=locales \
    --mount=type=cache,target=/var/cache/sccache \
    --mount=type=cache,target=/usr/local/cargo/git/db \
    --mount=type=cache,target=/usr/local/cargo/registry/ \
    cargo build --locked --release && \
    cp ./target/release/$APP_NAME /bin/server && \
    sccache --show-stats

################################################################################
FROM alpine:3.23 AS final

# Для работы с сетью и SSL в рантайме
RUN apk add --no-cache ca-certificates

ARG UID=10001
RUN adduser \
    --disabled-password \
    --gecos "" \
    --home "/nonexistent" \
    --shell "/sbin/nologin" \
    --no-create-home \
    --uid "${UID}" \
    appuser

WORKDIR /app

COPY --from=build /bin/server /bin/

COPY migrations ./migrations
COPY locales ./locales
COPY .sqlx ./.sqlx

USER appuser

EXPOSE 3050

CMD ["/bin/server"]
