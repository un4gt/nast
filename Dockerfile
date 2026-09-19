# syntax=docker/dockerfile:1
# 构建上下文 = nast 仓库；额外上下文 qqbot = ../qqbot-connector（docker-compose.yml 已配置）。
# 产物镜像含两个可执行文件：nast（服务端）/ nast-qqbot（QQ 机器人桥接）。

# ---------- 前端 ----------
FROM node:20-slim AS web
WORKDIR /web
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web/ ./
RUN npm run build

# ---------- Rust 构建 ----------
# 目录布局复刻本地：/build/{nast,qqbot-connector} 同级，
# 这样 crates/nast-qqbot 的 path = ../../../qqbot-connector 在容器内同样成立。
FROM rust:1-slim AS build
WORKDIR /build
# 命名上下文 qqbot（= ../qqbot-connector）；主上下文即 nast 仓库本身
COPY --from=qqbot / /build/qqbot-connector/
COPY . /build/nast/
WORKDIR /build/nast
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/nast/target \
    cargo build --release -p nast-server -p nast-qqbot \
    && cp target/release/nast target/release/nast-qqbot /usr/local/bin/

# ---------- 运行时 ----------
FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /usr/local/bin/nast /usr/local/bin/nast-qqbot /usr/local/bin/
COPY --from=web /web/dist /app/web/dist
COPY --from=build /build/nast/plugins /app/plugins
WORKDIR /app
ENV NAST_BIND=0.0.0.0 \
    NAST_PORT=8000 \
    NAST_DATA=/app/data \
    NAST_WEB=/app/web/dist
EXPOSE 8000
CMD ["nast"]
