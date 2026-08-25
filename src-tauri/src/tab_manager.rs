use crate::litebox::LiteBox;
use crate::tab_controller::TabController;
use crate::traits::TabOrchestrator;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tauri::{AppHandle, Manager, Runtime};

impl<R: Runtime> TabOrchestrator<R> for TabManager<R> {
    fn create_tab(&self, app: &AppHandle<R>, window_id: &str, url: &str) -> tauri::Result<String> {
        self.create_tab(app, window_id, url)
    }

    fn destroy_tab(&self, tab_id: &str) -> tauri::Result<()> {
        self.destroy_tab(tab_id)
    }

    fn show_tab(&self, tab_id: &str) -> tauri::Result<()> {
        let tabs = self
            .tabs
            .read()
            .expect("TabManager: tabs read lock poisoned");
        for (id, tab) in tabs.iter() {
            if id == tab_id {
                tab.show()?;
            } else {
                tab.hide()?;
            }
        }
        Ok(())
    }

    fn get_tab_ids(&self) -> Vec<String> {
        self.tabs
            .read()
            .expect("TabManager: tabs read lock poisoned")
            .keys()
            .cloned()
            .collect()
    }

    fn inject_theme_into_tab(
        &self,
        app: &AppHandle<R>,
        tab_id: &str,
        theme_name: &str,
    ) -> tauri::Result<()> {
        if let Some(tab) = self
            .tabs
            .read()
            .expect("TabManager: tabs read lock poisoned")
            .get(tab_id)
        {
            let theming = app.state::<Arc<dyn crate::traits::ThemingEngine<R>>>();
            theming.inject_theme(&tab.webview, theme_name);
        }
        Ok(())
    }
}

pub struct TabManager<R: Runtime> {
    pub tabs: RwLock<HashMap<String, Arc<TabController<R>>>>,
    pub litebox: Arc<LiteBox>,
}

impl<R: Runtime> TabManager<R> {
    pub fn new(litebox: Arc<LiteBox>) -> Self {
        Self {
            tabs: RwLock::new(HashMap::new()),
            litebox,
        }
    }

    pub fn create_tab(
        &self,
        app: &AppHandle<R>,
        window_id: &str,
        url: &str,
    ) -> tauri::Result<String> {
        let tab_id = uuid::Uuid::new_v4().to_string();

        {
            let state = app.state::<Arc<tokio::sync::Mutex<crate::state::AppState>>>();
            let mut state = state.blocking_lock();
            state.register_tab(
                window_id,
                crate::state::TabState {
                    id: tab_id.clone(),
                    title: "Notion".to_string(),
                    url: url.to_string(),
                    is_active: true,
                    is_pinned: false,
                },
            );
        }

        let tab_controller =
            match TabController::new(app, window_id, tab_id.clone(), url, self.litebox.clone()) {
                Ok(tab) => tab,
                Err(error) => {
                    let state = app.state::<Arc<tokio::sync::Mutex<crate::state::AppState>>>();
                    state.blocking_lock().remove_tab(&tab_id);
                    return Err(error);
                }
            };

        self.tabs
            .write()
            .expect("TabManager: tabs write lock poisoned")
            .insert(tab_id.clone(), Arc::new(tab_controller));

        let state = app.state::<Arc<tokio::sync::Mutex<crate::state::AppState>>>();
        let state = state.blocking_lock();
        if let Some(secret) = app.try_state::<Arc<Vec<u8>>>() {
            if let Err(error) = state.save_to_disk(secret.inner().as_slice()) {
                log::error!("Failed to save new tab state: {}", error);
            }
        }

        log::info!("TabManager: Created tab {}", tab_id);
        Ok(tab_id)
    }

    pub fn get_tab(&self, tab_id: &str) -> Option<String> {
        self.tabs
            .read()
            .expect("TabManager: tabs read lock poisoned")
            .get(tab_id)
            .map(|t| t.tab_id.clone())
    }

    pub fn destroy_tab(&self, tab_id: &str) -> tauri::Result<()> {
        if let Some(tab) = self
            .tabs
            .write()
            .expect("TabManager: tabs write lock poisoned")
            .remove(tab_id)
        {
            tab.destroy()?;
        }
        Ok(())
    }
}
