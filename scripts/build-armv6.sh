#!/bin/sh
# 在 Linux 上交叉编译 ARMv6（树莓派 Zero / Pi 1）静态 musl 版本。
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"

HOST=$(rustc -vV | sed -n 's/^host: //p')
LLD="$(rustc --print sysroot)/lib/rustlib/$HOST/bin/rust-lld"
if [ ! -x "$LLD" ]; then
  LLD=$(command -v ld.lld || true)
fi
if [ -z "$LLD" ] || [ ! -x "$LLD" ]; then
  echo "找不到 rust-lld 或 ld.lld，请安装 lld" >&2
  exit 1
fi

rustup target add arm-unknown-linux-musleabihf

echo "==> 构建 ARMv6 静态 musl 版本（linker: $LLD）"
CARGO_TARGET_ARM_UNKNOWN_LINUX_MUSLEABIHF_LINKER="$LLD" \
CARGO_TARGET_ARM_UNKNOWN_LINUX_MUSLEABIHF_RUSTFLAGS="-C link-arg=--threads=1" \
  cargo build --release --locked --target arm-unknown-linux-musleabihf

mkdir -p dist
cp target/arm-unknown-linux-musleabihf/release/samba-ui dist/samba-ui-armv6-musl
chmod 0755 dist/samba-ui-armv6-musl

if command -v sha256sum >/dev/null 2>&1; then
  (cd dist && sha256sum samba-ui-armv6-musl)
fi

ls -l dist/samba-ui-armv6-musl
