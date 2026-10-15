use std::sync::Arc;
use tauri::{AppHandle, Manager, Runtime};

/// デスクトップから渡されるNotionのリンクをHTTPSへ正規化する。
pub fn notion_url(value: &str) -> Option<String> {
    let normalized = if let Some(rest) = value.strip_prefix("notion://") {
        format!("https://{rest}")
    } else {
        value.to_string()
    };
    let url = url::Url::parse(&normalized).ok()?;
    let host = url.host_str()?;
    let notion = ["notion.so", "notion.com", "notion.site"]
        .iter()
        .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")));
    if url.scheme() == "https" && notion && url.username().is_empty() && url.password().is_none() {
        // Notionの紹介サイトへ転送される古いホームURLはログイン画面へ寄せる。
        if matches!(host, "notion.so" | "www.notion.so") && url.path() == "/" {
            return Some("https://app.notion.com/login".to_string());
        }
        Some(url.to_string())
    } else {
        None
    }
}

/// 起動済みのアプリでリンクを受け取った場合は新しいタブで開く。
pub fn open_from_args<R: Runtime>(app: &AppHandle<R>, args: &[String]) {
    let Some(url) = args.iter().skip(1).find_map(|arg| notion_url(arg)) else {
        return;
    };
    let app = app.clone();
    std::thread::spawn(move || {
        let Some(tabs) = app.try_state::<Arc<dyn crate::traits::TabOrchestrator<R>>>() else {
            return;
        };
        let Ok(id) = tabs.create_tab(&app, "main", &url) else {
            log::error!("Notionリンクを開けませんでした");
            return;
        };
        let _ = tabs.show_tab(&id);
        if let Some(state) = app.try_state::<Arc<tokio::sync::Mutex<crate::state::AppState>>>() {
            let mut state = state.blocking_lock();
            state.tabs.insert(
                id.clone(),
                crate::state::TabState {
                    id: id.clone(),
                    title: "Notion".into(),
                    url,
                    is_active: true,
                    is_pinned: false,
                },
            );
            if let Some(window) = state.windows.get_mut("main") {
                window.tab_ids.push(id.clone());
                window.active_tab_id = Some(id);
            }
            if let Some(secret) = app.try_state::<Arc<Vec<u8>>>() {
                let _ = state.save_to_disk(secret.as_slice());
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::notion_url;

    #[test]
    fn notion_protocol_and_https_are_supported() {
        assert_eq!(
            notion_url("https://www.notion.so").as_deref(),
            Some("https://app.notion.com/login")
        );
        assert_eq!(
            notion_url("notion://www.notion.so/page?pvs=4#section").as_deref(),
            Some("https://www.notion.so/page?pvs=4#section")
        );
        assert!(notion_url("https://team.notion.site/page").is_some());
    }

    #[test]
    fn unrelated_domains_and_protocols_are_rejected() {
        for url in [
            "https://notion.so.evil.example/page",
            "https://evilnotion.so",
            "https://notion.so@evil.example/",
            "http://www.notion.so",
            "javascript:alert(1)",
            "file:///etc/passwd",
        ] {
            assert!(notion_url(url).is_none());
        }
    }
}
