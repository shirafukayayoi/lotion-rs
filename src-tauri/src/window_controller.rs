use crate::traits::{SecuritySandbox, TabOrchestrator};
use std::sync::Arc;
use tauri::{
    AppHandle, LogicalPosition, LogicalSize, Manager, Runtime, WebviewBuilder, WebviewUrl, Window,
    WindowBuilder,
};

pub const TAB_BAR_HEIGHT: f64 = 42.0;
pub const TAB_BAR_LABEL: &str = "tab-strip";

pub struct WindowController<R: Runtime> {
    pub window: Window<R>,
    pub security: Arc<dyn SecuritySandbox>,
}

impl<R: Runtime> WindowController<R> {
    pub fn new(app: &AppHandle<R>, security: Arc<dyn SecuritySandbox>) -> tauri::Result<Self> {
        let window = WindowBuilder::new(app, "main")
            .title("lotion-rs")
            .inner_size(1200.0, 768.0)
            .decorations(cfg!(target_os = "windows"))
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
                log::error!(
                    "Zero-Trust: App secret not found in state when creating new window state."
                );
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
                if let Some(w) = app_handle.get_window(&window_label) {
                    if let Ok(scale_factor) = w.scale_factor() {
                        let logical_size = size.to_logical::<f64>(scale_factor);
                        for webview in w.webviews() {
                            if webview.label() == TAB_BAR_LABEL {
                                let _ = webview.set_position(LogicalPosition::new(0.0, 0.0));
                                let _ = webview.set_size(LogicalSize::new(
                                    logical_size.width,
                                    TAB_BAR_HEIGHT,
                                ));
                            } else {
                                let _ = webview
                                    .set_position(LogicalPosition::new(0.0, TAB_BAR_HEIGHT));
                                let _ = webview.set_size(LogicalSize::new(
                                    logical_size.width,
                                    (logical_size.height - TAB_BAR_HEIGHT).max(1.0),
                                ));
                            }
                        }
                    }
                }
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
        self.setup_tab_bar()?;

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
        let mut tabs_restored = false;

        if config.restore_tabs {
            let window_label = self.window.label();
            let (saved_tabs, saved_active_tab_id) = {
                let app_state_lock = app.state::<Arc<tokio::sync::Mutex<crate::state::AppState>>>();
                let mut app_state = app_state_lock.blocking_lock();
                let (old_tab_ids, active_tab_id) = app_state
                    .windows
                    .get(window_label)
                    .map(|window| (window.tab_ids.clone(), window.active_tab_id.clone()))
                    .unwrap_or_default();
                let saved_tabs = old_tab_ids
                    .iter()
                    .filter_map(|id| {
                        app_state
                            .tabs
                            .get(id)
                            .map(|tab| (id.clone(), tab.url.clone()))
                    })
                    .collect::<Vec<_>>();
                for id in &old_tab_ids {
                    app_state.tabs.remove(id);
                }
                if let Some(window) = app_state.windows.get_mut(window_label) {
                    window.tab_ids.clear();
                    window.active_tab_id = None;
                }
                (saved_tabs, active_tab_id)
            };

            log::info!(
                "WindowController: Restoring {} tabs from saved state.",
                saved_tabs.len()
            );
            let mut restored_ids = Vec::new();
            let mut active_id = None;
            for (old_id, url) in saved_tabs {
                match tab_manager.create_tab(app, window_label, &url) {
                    Ok(new_id) => {
                        if saved_active_tab_id.as_ref() == Some(&old_id) {
                            active_id = Some(new_id.clone());
                        }
                        restored_ids.push(new_id);
                        tabs_restored = true;
                    }
                    Err(error) => log::warn!("Failed to restore tab {}: {}", old_id, error),
                }
            }

            if let Some(active_id) = active_id.as_ref().or_else(|| restored_ids.last()) {
                tab_manager.show_tab(active_id)?;
                let app_state_lock = app.state::<Arc<tokio::sync::Mutex<crate::state::AppState>>>();
                let mut app_state = app_state_lock.blocking_lock();
                app_state.activate_tab(active_id);
                if let Some(secret) = app.try_state::<Arc<Vec<u8>>>() {
                    let _ = app_state.save_to_disk(secret.inner().as_slice());
                }
            }
        }

        if !tabs_restored {
            let notion_url = "https://www.notion.so";
            log::info!(
                "WindowController: Creating initial tab for Notion: {}",
                notion_url
            );
            let tab_id = tab_manager.create_tab(app, self.window.label(), notion_url)?;
            let _ = tab_manager.show_tab(&tab_id);
        }

        Ok(())
    }

    fn setup_tab_bar(&self) -> tauri::Result<()> {
        if self
            .window
            .webviews()
            .iter()
            .any(|webview| webview.label() == TAB_BAR_LABEL)
        {
            return Ok(());
        }

        let logical_size = self
            .window
            .inner_size()?
            .to_logical::<f64>(self.window.scale_factor()?);
        let builder = WebviewBuilder::new(TAB_BAR_LABEL, WebviewUrl::App("index.html".into()));
        self.window.add_child(
            builder,
            LogicalPosition::new(0.0, 0.0),
            LogicalSize::new(logical_size.width, TAB_BAR_HEIGHT),
        )?;
        Ok(())
    }
}
