# syntax=docker/dockerfile:1
# 构建上下文 = nast 仓库。IM 桥接（QQ/Discord/飞书）已拆分至 nast-bridges 仓库。

# ---------- 前端 ----------
FROM node:20-slim AS web
WORKDIR /web
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web/ ./
COPY resources/ /resources/
RUN npm run build

# ---------- Rust 构建 ----------
FROM rust:1-slim-bookworm AS build
WORKDIR /build/nast
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/nast/target \
    cargo build --release -p nast-server \
    && cp target/release/nast /usr/local/bin/

# ---------- 运行时 ----------
FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /usr/local/bin/nast /usr/local/bin/
COPY --from=web /web/dist /app/web/dist
COPY --from=build /build/nast/plugins /app/plugins
COPY resources/NOTICE.md /app/resources/NOTICE.md
WORKDIR /app
ENV NAST_BIND=0.0.0.0 \
    NAST_PORT=8000 \
    NAST_DATA=/app/data \
    NAST_WEB=/app/web/dist
EXPOSE 8000
CMD ["nast"]
