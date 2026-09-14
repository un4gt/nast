#!/usr/bin/env bash
# nast 一键启动：构建前端（如缺失）+ 启动服务
set -e
cd "$(dirname "$0")"

if [ ! -f web/dist/index.html ]; then
    echo "[run] web/dist 不存在，构建前端…"
    (cd web && npm install && npm run build)
fi

echo "[run] 启动 nast（http://127.0.0.1:${NAST_PORT:-8000}）"
exec cargo run
