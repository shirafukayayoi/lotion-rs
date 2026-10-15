# このforkのLinux向け導入

対象は `shirafukayayoi/lotion-rs` の `master` をベースにしたLinux対応です。Rust／Tauri v2とシステムのWebKitGTKを使用します。Chromiumは同梱しません。NotionのWeb画面を表示するため、ログインは通常のNotionと同じです。Notion APIトークンの入力は不要です。

## Arch Linuxでの依存関係

このPCでは次の依存パッケージは導入済みです。別のPCでは、配布元のパッケージマネージャーで用意してください。

```sh
sudo pacman -S --needed base-devel rust gtk3 webkit2gtk-4.1 libayatana-appindicator librsvg hunspell hunspell-en_us gst-plugins-good
```

## ビルドとユーザー領域への導入

リポジトリー直下で実行します。

```sh
cargo test --manifest-path src-tauri/Cargo.toml --release --locked --lib
./scripts/install-linux.sh
```

管理者権限は不要です。アプリ一覧へ **Lotion-rs** を登録し、`notion://`リンクを関連付けます。ビルド済みの実行ファイルを渡すこともできます。

```sh
./scripts/install-linux.sh /path/to/release/lotion-rs
```

配置先:

- 実行ファイル: `~/.local/opt/lotion-rs/bin/lotion-rs`
- リソース: `~/.local/opt/lotion-rs/lib/lotion-rs/`
- 起動コマンド: `~/.local/bin/lotion-rs`
- 設定: `~/.config/lotion-rs/`
- WebKitのログインデータ: `~/.local/share/io.lotion-rs.secure.v2/`

既存のLotion-rs設定とログインデータは保持します。更新時は同じインストールスクリプトを再実行してください。

## Linux向けの変更

- 起動時に親プロセスのネットワーク名前空間を切断する処理を廃止。一般ユーザーでも適用できる権限制限とWebKitGTKのサンドボックスを使用します。
- 起動時は現在のNotionログイン画面 `https://app.notion.com/login` を開きます。古いホームURLを復元する場合も同じ画面へ進みます。
- LinuxではOS側のウィンドウ枠と閉じる・最小化・最大化操作を使用します。
- Linuxでは「Edit」「View」などのアプリ側のメニューバーを表示しません。
- WebKitGTKが作る `about:blank` 初期文書を内部文書として扱います。その他の `about:` ページや外部への不正なプロトコルは許可しません。
- `notion://`とNotionのHTTPSリンクを受け取り、起動時・起動済みの場合の両方で対象ページを開きます。リンクはNotionのドメインに限定します。
- ユーザー領域に実行ファイル・リソース・アイコン・デスクトップ項目を配置するスクリプトを追加しました。

## Waylandとログ

WaylandセッションではWaylandバックエンドを選択します。X11を使いたい場合は次のように起動できます。

```sh
LOTION_GDK_BACKEND=x11 lotion-rs
```

詳細な起動ログが必要な場合:

```sh
RUST_LOG=info lotion-rs
```

WebKitのサンドボックスを無効にする設定では起動しません。通常のユーザー権限のまま起動してください。
