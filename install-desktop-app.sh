#!/usr/bin/env bash
set -e

DESKTOP_DIR="$HOME/.local/share/applications"
mkdir -p "$DESKTOP_DIR"

cp /home/ivan/local_helper_ai/local-helper-ai.desktop "$DESKTOP_DIR/local-helper-ai.desktop"
chmod +x "$DESKTOP_DIR/local-helper-ai.desktop"

if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$DESKTOP_DIR" 2>/dev/null || true
fi

echo "============================================================"
echo "✅ Local Helper Mascot успішно додано у ваш список програм!"
echo "   Тепер ви можете шукати його у меню програм (Super / Windows key)"
echo "   і запускати в 1 клік без терміналу!"
echo "============================================================"

