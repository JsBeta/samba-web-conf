# 在 Windows 上用本地 ARM GNU Toolchain 交叉编译 ARMv7 版本。
# 用法: .\scripts\build-armv7.ps1 [-Toolchain <ARM GNU Toolchain 根目录>]

param(
    [string]$Toolchain = 'E:\arm-gnu-toolchain-13.3.rel1-mingw-w64-i686-arm-none-linux-gnueabihf'
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    $sysroot = (rustc --print sysroot).Trim()
    $hostTriple = ((rustc -vV) | Where-Object { $_ -like 'host:*' }) -replace '^host:\s*', ''
    $lld = Join-Path $sysroot "lib\rustlib\$hostTriple\bin\rust-lld.exe"
    $gcc = Join-Path $Toolchain 'bin\arm-none-linux-gnueabihf-gcc.exe'

    if (!(Test-Path $lld)) { throw "找不到 rust-lld: $lld" }
    if (!(Test-Path $gcc)) { throw "找不到 ARM GCC: $gcc" }

    Write-Host "==> 检查 Rust 目标"
    rustup target add armv7-unknown-linux-musleabihf armv7-unknown-linux-gnueabihf
    if ($LASTEXITCODE -ne 0) { throw "rustup target add 失败" }

    Write-Host "==> 构建 musl 静态版本"
    $env:PATH = "$(Split-Path $lld);$env:PATH"
    $env:CARGO_TARGET_ARMV7_UNKNOWN_LINUX_MUSLEABIHF_LINKER = $lld
    # 该版本 rust-lld 在部分 Windows 主机上多线程链接会崩溃，固定单线程。
    $env:CARGO_TARGET_ARMV7_UNKNOWN_LINUX_MUSLEABIHF_RUSTFLAGS = '-C link-arg=--threads=1'
    cargo build --release --target armv7-unknown-linux-musleabihf
    if ($LASTEXITCODE -ne 0) { throw "musl 构建失败" }

    Write-Host "==> 构建 glibc 动态版本"
    $env:CARGO_TARGET_ARMV7_UNKNOWN_LINUX_GNUEABIHF_LINKER = $gcc
    cargo build --release --target armv7-unknown-linux-gnueabihf
    if ($LASTEXITCODE -ne 0) { throw "glibc 构建失败" }

    $dist = Join-Path $root 'dist'
    New-Item -ItemType Directory -Force -Path $dist | Out-Null
    Copy-Item "target\armv7-unknown-linux-musleabihf\release\samba-ui" (Join-Path $dist 'samba-ui-armv7-musl') -Force
    Copy-Item "target\armv7-unknown-linux-gnueabihf\release\samba-ui" (Join-Path $dist 'samba-ui-armv7-glibc') -Force

    Write-Host "==> 产物"
    Get-ChildItem $dist -Filter 'samba-ui-armv7-*' | ForEach-Object {
        $hash = (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower()
        "{0}  {1}  {2} bytes" -f $hash, $_.Name, $_.Length
    }
} finally {
    Pop-Location
}
