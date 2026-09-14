# nast 一键启动：构建前端（如缺失）+ 启动服务
$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

if (-not (Test-Path "web/dist/index.html")) {
    Write-Host "[run] web/dist 不存在，构建前端…"
    Push-Location web
    npm install
    npm run build
    Pop-Location
}

$port = if ($env:NAST_PORT) { $env:NAST_PORT } else { "8000" }
Write-Host "[run] 启动 nast（http://127.0.0.1:$port）"
cargo run
