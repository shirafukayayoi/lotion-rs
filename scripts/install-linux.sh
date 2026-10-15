#!/usr/bin/env bash
set -euo pipefail
repo_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
if [[ -n "${1:-}" ]]; then
    binary_path=$1
else
    cargo build --manifest-path "$repo_dir/src-tauri/Cargo.toml" --release --locked
    binary_path="${CARGO_TARGET_DIR:-$repo_dir/src-tauri/target}/release/lotion-rs"
fi
[[ -x "$binary_path" ]] || { echo 'ビルド済みの実行ファイルが見つかりません。' >&2; exit 1; }
app_dir="$HOME/.local/opt/lotion-rs"
bin_dir="$HOME/.local/bin"
desktop_dir="$HOME/.local/share/applications"
icon_dir="$HOME/.local/share/icons/hicolor/256x256/apps"
mkdir -p "$app_dir/bin" "$app_dir/lib/lotion-rs" "$bin_dir" "$desktop_dir" "$icon_dir"
install -m755 "$binary_path" "$app_dir/bin/lotion-rs.new"
mv "$app_dir/bin/lotion-rs.new" "$app_dir/bin/lotion-rs"
cp -R "$repo_dir/src-tauri/assets" "$repo_dir/src-tauri/i18n" "$app_dir/lib/lotion-rs/"
install -m644 "$repo_dir/src-tauri/assets/icon.png" "$icon_dir/lotion-rs.png"
cat > "$bin_dir/lotion-rs" <<'LAUNCHER'
#!/bin/sh
if [ -n "${WAYLAND_DISPLAY:-}" ]; then
    export GDK_BACKEND="${LOTION_GDK_BACKEND:-wayland}"
fi
export RUST_LOG="${RUST_LOG:-warn}"
exec "$HOME/.local/opt/lotion-rs/bin/lotion-rs" "$@"
LAUNCHER
chmod 755 "$bin_dir/lotion-rs"
cat > "$desktop_dir/lotion-rs.desktop" <<DESKTOP
[Desktop Entry]
Version=1.0
Type=Application
Name=Lotion-rs
Comment=NotionのLinux用デスクトップクライアント
Exec="$bin_dir/lotion-rs" %U
Icon=lotion-rs
Terminal=false
Categories=Office;
StartupWMClass=lotion-rs
MimeType=x-scheme-handler/notion;
DESKTOP
desktop-file-validate "$desktop_dir/lotion-rs.desktop"
update-desktop-database "$desktop_dir"
xdg-mime default lotion-rs.desktop x-scheme-handler/notion
printf 'Lotion-rsをインストールしました。アプリ一覧から起動できます。\n'
