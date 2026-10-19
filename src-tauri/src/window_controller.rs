use crate::traits::{SecuritySandbox, TabOrchestrator};
use std::sync::Arc;
use tauri::{AppHandle, Manager, Runtime, Window, WindowBuilder};

pub struct WindowController<R: Runtime> {
    pub window: Window<R>,
    pub security: Arc<dyn SecuritySandbox>,
}

impl<R: Runtime> WindowController<R> {
    pub fn new(app: &AppHandle<R>, security: Arc<dyn SecuritySandbox>) -> tauri::Result<Self> {
        let window = WindowBuilder::new(app, "main")
            .title("lotion-rs")
            .inner_size(1200.0, 768.0)
            .decorations(app.state::<crate::config::LotionConfig>().window.native_titlebar)
            .build()?;

        // Ensure window state exists in AppState
        let app_state_lock = app.state::<Arc<tokio::sync::Mutex<crate::state::AppState>>>();
        let mut app_state = app_state_lock.blocking_lock();
        if !app_state.windows.contains_key("main") {
            app_state.windows.insert(
                "main".to_string(),
                crate::state::WindowState {
                    id: "main".to_string(),
                    bounds: crate::state::Bounds {
                        x: None,
                        y: None,
                        width: 1200.0,
                        height: 768.0,
                    },
                    is_focused: true,
                    is_maximized: false,
                    is_minimized: false,
                    is_full_screen: false,
                    tab_ids: Vec::new(),
                    active_tab_id: None,
                },
            );
            if let Some(app_secret_state) = app.try_state::<Arc<Vec<u8>>>() {
                let _ = app_state.save_to_disk(app_secret_state.inner().as_slice());
            } else {
                log::error!("Zero-Trust: App secret not found in state when creating new window state.");
            }
        }

        Ok(Self { window, security })
    }

    pub fn setup_listeners(&self, app_handle: AppHandle<R>) {
        let window_label = self.window.label().to_string();

        self.window.on_window_event(move |event| match event {
            tauri::WindowEvent::CloseRequested { .. } => {
                log::info!("Window {} close requested", window_label);
                app_handle.exit(0);
            }
            tauri::WindowEvent::Focused(focused) => {
                log::debug!("Window {} focused: {}", window_label, focused);
                let app_state_lock =
                    app_handle.state::<Arc<tokio::sync::Mutex<crate::state::AppState>>>();
                let mut app_state = app_state_lock.blocking_lock();
                if *focused {
                    app_state.focused_window_id = Some(window_label.clone());
                }
                if let Some(w_state) = app_state.windows.get_mut(&window_label) {
                    w_state.is_focused = *focused;
                }
                if let Some(app_secret_state) = app_handle.try_state::<Arc<Vec<u8>>>() {
                    let _ = app_state.save_to_disk(app_secret_state.inner().as_slice());
                } else {
                    log::error!("Zero-Trust: App secret not found in state when saving AppState (focused event).");
                }
            }
            tauri::WindowEvent::Resized(size) => {
                log::debug!("Window {} resized to {:?}", window_label, size);
                // WebviewBuilderのauto_resizeで表示領域を親ウィンドウへ追従させる。
                let app_state_lock =
                    app_handle.state::<Arc<tokio::sync::Mutex<crate::state::AppState>>>();
                let mut app_state = app_state_lock.blocking_lock();
                if let Some(w_state) = app_state.windows.get_mut(&window_label) {
                    w_state.bounds.width = size.width as f64;
                    w_state.bounds.height = size.height as f64;
                }
                if let Some(app_secret_state) = app_handle.try_state::<Arc<Vec<u8>>>() {
                    let _ = app_state.save_to_disk(app_secret_state.inner().as_slice());
                } else {
                    log::error!("Zero-Trust: App secret not found in state when saving AppState (resized event).");
                }
            }
            tauri::WindowEvent::Moved(position) => {
                log::debug!("Window {} moved to {:?}", window_label, position);
                let app_state_lock =
                    app_handle.state::<Arc<tokio::sync::Mutex<crate::state::AppState>>>();
                let mut app_state = app_state_lock.blocking_lock();
                if let Some(w_state) = app_state.windows.get_mut(&window_label) {
                    w_state.bounds.x = Some(position.x as f64);
                    w_state.bounds.y = Some(position.y as f64);
                }
                if let Some(app_secret_state) = app_handle.try_state::<Arc<Vec<u8>>>() {
                    let _ = app_state.save_to_disk(app_secret_state.inner().as_slice());
                } else {
                    log::error!("Zero-Trust: App secret not found in state when saving AppState (moved event).");
                }
            }
            _ => {}
        });
    }

    pub fn setup_tabs(&self, app: &AppHandle<R>) -> tauri::Result<()> {
        let tab_manager = {
            let mut attempts = 0;
            loop {
                if let Some(state) = app.try_state::<Arc<dyn TabOrchestrator<R>>>() {
                    break state;
                }
                attempts += 1;
                if attempts > 60 {
                    log::error!("WindowController: TabOrchestrator state not available after 3s");
                    return Err(tauri::Error::AssetNotFound(
                        "TabOrchestrator state timeout".into(),
                    ));
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        };

        let config = app.state::<crate::config::LotionConfig>();
        let launch_url = std::env::args().skip(1).find_map(|arg| crate::launch::notion_url(&arg));
        // 状態の読み取りと画面作成を分離する。ロック中にUIを呼ぶと、
        // メインスレッドのフォーカス／リサイズイベントと待ち合ってしまう。
        let state_lock = app.state::<Arc<tokio::sync::Mutex<crate::state::AppState>>>();
        let (restored_tabs, old_active) = {
            let state = state_lock.blocking_lock();
            if config.restore_tabs && launch_url.is_none() {
                if let Some(window) = state.windows.get(self.window.label()) {
                    let tabs = window.tab_ids.iter()
                        .filter_map(|id| state.tabs.get(id).cloned())
                        .collect::<Vec<_>>();
                    (tabs, window.active_tab_id.clone())
                } else {
                    (Vec::new(), None)
                }
            } else {
                (Vec::new(), None)
            }
        };

        let source_tabs = if restored_tabs.is_empty() {
            vec![crate::state::TabState {
                id: String::new(),
                title: "Notion".into(),
                url: launch_url.unwrap_or_else(|| "https://app.notion.com/login".into()),
                is_active: true,
                is_pinned: false,
            }]
        } else {
            restored_tabs
        };
        let mut created_tabs = Vec::new();
        let mut active_id = None;
        for mut tab in source_tabs {
            let url = crate::launch::notion_url(&tab.url).unwrap_or(tab.url.clone());
            let id = tab_manager.create_tab(app, self.window.label(), &url)?;
            if old_active.as_ref() == Some(&tab.id) || (old_active.is_none() && tab.is_active) {
                active_id = Some(id.clone());
            }
            tab.id = id;
            tab.url = url;
            created_tabs.push(tab);
        }
        let active_id = active_id.or_else(|| created_tabs.last().map(|tab| tab.id.clone()));
        {
            let mut state = state_lock.blocking_lock();
            let ids = created_tabs.iter().map(|tab| tab.id.clone()).collect::<Vec<_>>();
            if let Some(window) = state.windows.get_mut(self.window.label()) {
                let old_ids = std::mem::replace(&mut window.tab_ids, ids);
                window.active_tab_id = active_id.clone();
                for id in old_ids {
                    state.tabs.remove(&id);
                }
            }
            for mut tab in created_tabs {
                tab.is_active = active_id.as_ref() == Some(&tab.id);
                state.tabs.insert(tab.id.clone(), tab);
            }
            if let Some(secret) = app.try_state::<Arc<Vec<u8>>>() {
                let _ = state.save_to_disk(secret.inner().as_slice());
            }
        }
        if let Some(id) = active_id {
            tab_manager.show_tab(&id)?;
        }

        Ok(())
    }
}
