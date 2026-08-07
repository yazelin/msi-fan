#!/bin/bash
# 安裝到使用者目錄，不需要 root。
# 但控制項的權限設定需要 root，那一步在 README 裡另外說明。
set -e
cd "$(dirname "$0")"
mkdir -p ~/.local/bin ~/.local/share/applications ~/.local/share/icons/hicolor/256x256/apps ~/.config/systemd/user

cp src-tauri/target/release/msi-fan ~/.local/bin/
cp dist/msi-fan.desktop ~/.local/share/applications/
cp src-tauri/icons/256x256.png ~/.local/share/icons/hicolor/256x256/apps/msi-fan.png
cp dist/msi-fan-auto.service ~/.config/systemd/user/

systemctl --user daemon-reload
update-desktop-database ~/.local/share/applications 2>/dev/null || true

echo "已安裝。"
echo "  啟動程式：msi-fan（或從應用程式選單找 MSI Fan）"
echo "  背景自動控制：systemctl --user enable --now msi-fan-auto"
