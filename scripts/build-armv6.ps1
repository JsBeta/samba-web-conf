# 在 Windows 上交叉编译 ARMv6（树莓派 Zero / Pi 1）静态 musl 版本。
# 用法: .\scripts\build-armv6.ps1

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    $sysroot = (rustc --print sysroot).Trim()
    $hostTriple = ((rustc -vV) | Where-Object { $_ -like 'host:*' }) -replace '^host:\s*', ''
    $lld = Join-Path $sysroot "lib\rustlib\$hostTriple\bin\rust-lld.exe"
    if (!(Test-Path $lld)) { throw "找不到 rust-lld: $lld" }

    Write-Host "==> 安装 ARMv6 musl 目标"
    rustup target add arm-unknown-linux-musleabihf
    if ($LASTEXITCODE -ne 0) { throw "rustup target add 失败" }

    Write-Host "==> 构建 ARMv6 静态 musl 版本"
    $env:PATH = "$(Split-Path $lld);$env:PATH"
    $env:CARGO_TARGET_ARM_UNKNOWN_LINUX_MUSLEABIHF_LINKER = $lld
    # 该版本 rust-lld 在部分 Windows 主机上多线程链接会崩溃，固定单线程。
    $env:CARGO_TARGET_ARM_UNKNOWN_LINUX_MUSLEABIHF_RUSTFLAGS = '-C link-arg=--threads=1'
    cargo build --release --locked --target arm-unknown-linux-musleabihf
    if ($LASTEXITCODE -ne 0) { throw "ARMv6 构建失败" }

    $dist = Join-Path $root 'dist'
    New-Item -ItemType Directory -Force -Path $dist | Out-Null
    Copy-Item "target\arm-unknown-linux-musleabihf\release\samba-ui" (Join-Path $dist 'samba-ui-armv6-musl') -Force

    $file = Join-Path $dist 'samba-ui-armv6-musl'
    $hash = (Get-FileHash $file -Algorithm SHA256).Hash.ToLower()
    $size = (Get-Item $file).Length
    "{0}  {1}  {2} bytes" -f $hash, 'samba-ui-armv6-musl', $size
} finally {
    Pop-Location
}
