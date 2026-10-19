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
- Linuxでは独立したタイトルバーを省き、Notionの表示領域を広く使います。移動・サイズ変更・終了はウィンドウマネージャーの操作を使用してください。
- Linuxでは「Edit」「View」などのアプリ側のメニューバーを表示しません。
- 初期配色はNotionの設定に従います。Draculaの背景上書きはダーク表示の範囲に限定し、ライト表示の文字が暗い背景に埋もれる問題を修正しました。
- ページ名をLinuxのウィンドウタイトルへ反映し、HiDPI画面での表示サイズとウィンドウへの追従を修正しました。
- タブ復元時は状態のロックを解放してから画面を作成し、起動時にフォーカスやリサイズイベントと待ち合う問題を防ぎます。
- WebKitGTKが作る `about:blank` 初期文書を内部文書として扱います。その他の `about:` ページや外部への不正なプロトコルは許可しません。
- `notion://`とNotionのHTTPSリンクを受け取り、起動時・起動済みの場合の両方で対象ページを開きます。リンクはNotionのドメインに限定します。
- ユーザー領域に実行ファイル・リソース・アイコン・デスクトップ項目を配置するスクリプトを追加しました。

## Waylandとログ

閉じるボタンなどのOS標準タイトルバーが必要な場合は、`~/.config/lotion-rs/config.toml` の `[window]` に `native_titlebar = true` を設定して再起動してください。標準配色を使う場合は、ファイル先頭の `active_theme = "default"` を指定します。明暗の切り替えはNotionの設定で行えます。

WaylandセッションではWaylandバックエンドを選択します。X11を使いたい場合は次のように起動できます。

```sh
LOTION_GDK_BACKEND=x11 lotion-rs
```

詳細な起動ログが必要な場合:

```sh
RUST_LOG=info lotion-rs
```

WebKitのサンドボックスを無効にする設定では起動しません。通常のユーザー権限のまま起動してください。
