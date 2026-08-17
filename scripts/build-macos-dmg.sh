#!/usr/bin/env bash
set -euo pipefail

if [[ "${OSTYPE:-}" != darwin* ]]; then
  echo "This script must run on macOS." >&2
  exit 1
fi

APP_PATH="${1:?usage: build-macos-dmg.sh /path/to/Quota Pro.app VERSION [OUTPUT_DIR]}"
VERSION="${2:?usage: build-macos-dmg.sh /path/to/Quota Pro.app VERSION [OUTPUT_DIR]}"
OUTPUT_DIR="${3:-$(dirname "$APP_PATH")}"

if [[ ! -d "$APP_PATH" ]]; then
  echo "App bundle not found: $APP_PATH" >&2
  exit 1
fi

APP_NAME="Quota Pro"
WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/quota-pro-dmg.XXXXXX")"
STAGE_DIR="$WORK_DIR/Quota Pro"
COMPONENT_APP="$WORK_DIR/$APP_NAME.app"
DMG_PATH="$OUTPUT_DIR/Quota Pro_${VERSION}_universal-installer.dmg"
PKG_PATH="$STAGE_DIR/Quota Pro.pkg"

cleanup() { rm -rf "$WORK_DIR"; }
trap cleanup EXIT

mkdir -p "$STAGE_DIR" "$OUTPUT_DIR"
ditto "$APP_PATH" "$COMPONENT_APP"

# A component package is Apple's standard installer format and installs into
# /Applications without requiring a third-party installer application.
/usr/bin/pkgbuild \
  --component "$COMPONENT_APP" \
  --install-location /Applications \
  "$PKG_PATH" >/dev/null

cp "$(dirname "$0")/remove-quarantine.command" "$STAGE_DIR/移除文件已损坏.command"
chmod +x "$STAGE_DIR/移除文件已损坏.command"

cat > "$STAGE_DIR/安装说明.txt" <<'EOF'
Quota Pro 安装

1. 双击“Quota Pro.pkg”进行标准安装。
2. 如果 macOS 提示“文件已损坏”，先双击“移除文件已损坏.command”，再打开应用。

该脚本只移除下载隔离属性，不关闭 macOS 签名校验，也不会绕过应用本身的签名。
EOF

/usr/bin/hdiutil create \
  -volname "Quota Pro $VERSION" \
  -srcfolder "$STAGE_DIR" \
  -ov \
  -format UDZO \
  "$DMG_PATH" >/dev/null

echo "Created $DMG_PATH"
