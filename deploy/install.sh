#!/bin/sh
# 在目标 ARMv7 设备上安装 samba-ui 与 systemd unit。
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
BIN="${1:-$ROOT/dist/samba-ui-armv7-musl}"
UNIT_SRC="$ROOT/deploy/samba-ui.service"
ENV_SRC="$ROOT/deploy/samba-ui.env.example"
BIN_DST="${BIN_DST:-/usr/local/bin/samba-ui}"
UNIT_DST="${UNIT_DST:-/etc/systemd/system/samba-ui.service}"
ENV_DST="${ENV_DST:-/etc/samba/samba-ui.env}"

if [ ! -f "$BIN" ]; then
  echo "找不到二进制: $BIN" >&2
  echo "用法: sh deploy/install.sh [二进制路径]" >&2
  exit 1
fi
if [ "$(id -u)" -ne 0 ]; then
  echo "请用 root 运行（需要写 /etc）" >&2
  exit 1
fi

install -d "$(dirname "$BIN_DST")"
install -m 0755 "$BIN" "$BIN_DST"
install -m 0644 "$UNIT_SRC" "$UNIT_DST"

if [ -f "$ROOT/README.md" ]; then
  install -d -m 0755 /usr/local/share/doc/samba-ui
  install -m 0644 "$ROOT/README.md" /usr/local/share/doc/samba-ui/README.md
fi

if [ ! -f "$ENV_DST" ]; then
  install -d -m 0750 "$(dirname "$ENV_DST")"
  install -m 0600 "$ENV_SRC" "$ENV_DST"
  echo "已创建 $ENV_DST"
  echo "请先修改其中的 SAMBA_UI_PASSWORD，再执行：systemctl enable --now samba-ui"
else
  echo "保留已有配置 $ENV_DST"
fi

systemctl daemon-reload
echo "安装完成。启动：systemctl enable --now samba-ui"
