use tauri::{Runtime, Webview};
use webkit2gtk::{CacheModel, WebContextExt, WebViewExt};

/// ページやリソースのキャッシュを、必要な分を残して控えめにする。
/// Cookie・ローカルストレージ・編集中の文書は削除しない。
pub fn configure<R: Runtime>(webview: &Webview<R>, low_memory: bool) -> tauri::Result<()> {
    if !low_memory {
        return Ok(());
    }
    webview.with_webview(|platform| {
        let view = platform.inner();
        if let Some(context) = view.context() {
            context.set_cache_model(CacheModel::DocumentBrowser);
        }
        log::debug!("WebKitGTKの省メモリ設定を適用しました");
    })
}
