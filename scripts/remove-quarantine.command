#!/bin/zsh
# Quota Pro helper: remove macOS quarantine metadata from the installer included
# in this disk image, then open the standard installer. This does not disable
# signing checks or install anything without the user's confirmation.

set -u

SCRIPT_DIR="${0:A:h}"
PKG_PATH="$SCRIPT_DIR/Quota Pro.pkg"

if [[ ! -f "$PKG_PATH" ]]; then
  print -r -- "找不到 Quota Pro.pkg，请从 DMG 内运行此脚本。"
  read -r "REPLY?按回车键退出..."
  exit 1
fi

/usr/bin/xattr -d com.apple.quarantine "$PKG_PATH" 2>/dev/null || true

print -r -- "已移除安装包的下载隔离属性。现在打开 Quota Pro 安装器。"
/usr/bin/open "$PKG_PATH"
read -r "REPLY?按回车键关闭此窗口..."
