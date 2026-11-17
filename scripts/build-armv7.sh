#!/bin/sh
# 在 Linux/macOS 上交叉编译 ARMv7 版本。
# 需要 arm-linux-gnueabihf-gcc（glibc 版本）和一个 musl 链接器。
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"

: "${ARM_GCC:=arm-linux-gnueabihf-gcc}"
: "${MUSL_LINKER:=}"

if [ -z "$MUSL_LINKER" ]; then
  HOST=$(rustc -vV | sed -n 's/^host: //p')
  CANDIDATE="$(rustc --print sysroot)/lib/rustlib/$HOST/bin/rust-lld"
  if [ -x "$CANDIDATE" ]; then
    MUSL_LINKER="$CANDIDATE"
  else
    MUSL_LINKER=rust-lld
  fi
fi

rustup target add armv7-unknown-linux-musleabihf armv7-unknown-linux-gnueabihf

echo "==> musl 静态版本（linker: $MUSL_LINKER）"
CARGO_TARGET_ARMV7_UNKNOWN_LINUX_MUSLEABIHF_LINKER="$MUSL_LINKER" \
  cargo build --release --target armv7-unknown-linux-musleabihf

echo "==> glibc 动态版本（linker: $ARM_GCC）"
CARGO_TARGET_ARMV7_UNKNOWN_LINUX_GNUEABIHF_LINKER="$ARM_GCC" \
  cargo build --release --target armv7-unknown-linux-gnueabihf

mkdir -p dist
cp target/armv7-unknown-linux-musleabihf/release/samba-ui dist/samba-ui-armv7-musl
cp target/armv7-unknown-linux-gnueabihf/release/samba-ui dist/samba-ui-armv7-glibc
chmod 0755 dist/samba-ui-armv7-musl dist/samba-ui-armv7-glibc

if command -v sha256sum >/dev/null 2>&1; then
  (cd dist && sha256sum samba-ui-armv7-musl samba-ui-armv7-glibc > SHA256SUMS)
fi

ls -l dist
