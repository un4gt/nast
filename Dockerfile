# syntax=docker/dockerfile:1
# 构建上下文 = nast-bridges 仓库；额外上下文 qqbot = ../qqbot-connector。
# 一个镜像，含 bridge-qq / bridge-discord / bridge-feishu 三个二进制。

FROM rust:1-slim AS build
WORKDIR /build
COPY --from=qqbot / /build/qqbot-connector/
COPY . /build/nast-bridges/
WORKDIR /build/nast-bridges
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/nast-bridges/target \
    cargo build --release -p bridge-qq -p bridge-discord -p bridge-feishu \
    && cp target/release/bridge-qq target/release/bridge-discord target/release/bridge-feishu /usr/local/bin/

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /usr/local/bin/bridge-qq /usr/local/bin/bridge-discord /usr/local/bin/bridge-feishu /usr/local/bin/
WORKDIR /app
CMD ["bridge-qq"]
