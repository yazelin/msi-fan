#!/bin/bash
# 安裝到使用者目錄，不需要 root。
# 但控制項的權限設定需要 root，那一步在 README 裡另外說明。
set -e
cd "$(dirname "$0")"
mkdir -p ~/.local/bin ~/.local/share/applications ~/.local/share/icons/hicolor/256x256/apps ~/.config/systemd/user

# 守護行程正在執行這個執行檔的話，直接覆蓋會得到「Text file busy」。
# 先停、複製完再依原狀態決定要不要啟回來。
WAS_ACTIVE=$(systemctl --user is-active msi-fan-auto 2>/dev/null || true)
[ "$WAS_ACTIVE" = "active" ] && systemctl --user stop msi-fan-auto
# GUI 開著一樣會讓 cp 拿到 Text file busy。pkill 是非同步的，要等它真的死透，
# 不然 cp 會搶在行程結束前執行，錯誤訊息還是一樣。
pkill -x msi-fan 2>/dev/null || true
for _ in $(seq 20); do pgrep -x msi-fan >/dev/null || break; sleep 0.2; done
cp src-tauri/target/release/msi-fan ~/.local/bin/
cp dist/msi-fan.desktop ~/.local/share/applications/
cp src-tauri/icons/256x256.png ~/.local/share/icons/hicolor/256x256/apps/msi-fan.png
cp dist/msi-fan-auto.service ~/.config/systemd/user/

systemctl --user daemon-reload
update-desktop-database ~/.local/share/applications 2>/dev/null || true

[ "$WAS_ACTIVE" = "active" ] && systemctl --user start msi-fan-auto

echo "已安裝。"
echo "  啟動程式：msi-fan（或從應用程式選單找 MSI Fan）"
echo "  背景自動控制：systemctl --user enable --now msi-fan-auto"
