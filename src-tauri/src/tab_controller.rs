use crate::litebox::LiteBox;
use crate::traits::{PolicyEnforcer, ThemingEngine};
use std::sync::Arc;
use tauri::webview::Webview;
use tauri::{AppHandle, Manager, Runtime, Url, WebviewBuilder, WebviewUrl};

pub struct TabController<R: Runtime> {
    pub tab_id: String,
    pub window_id: String,
    pub webview: Webview<R>,
}

impl<R: Runtime> TabController<R> {
    pub fn new(
        app: &AppHandle<R>,
        window_id: &str,
        tab_id: String,
        url_str: &str,
        _litebox: Arc<LiteBox>,
    ) -> tauri::Result<Self> {
        let policy = app.state::<Arc<dyn PolicyEnforcer>>().inner().clone();

        // Zero-Trust Enforcement: Validate URL before creation
        if !policy.validate_url(url_str) {
            return Err(tauri::Error::AssetNotFound(format!(
                "Zero-Trust Policy Blocked: {}",
                url_str
            )));
        }

        let window = app
            .get_window(window_id)
            .ok_or(tauri::Error::AssetNotFound(format!(
                "Window {} not found",
                window_id
            )))?;

        let url = url_str
            .parse::<Url>()
            .map_err(|e| tauri::Error::AssetNotFound(e.to_string()))?;

        // Create a new webview for this tab using the secure factory
        let webview_builder =
            create_secure_webview_builder(app, &tab_id, &url, window_id, policy.clone());

        let inner_size = window
            .inner_size()?
            .to_logical::<f64>(window.scale_factor()?);
        let webview = window.add_child(
            webview_builder,
            tauri::LogicalPosition::new(0.0, crate::window_controller::TAB_BAR_HEIGHT),
            tauri::LogicalSize::new(
                inner_size.width,
                (inner_size.height - crate::window_controller::TAB_BAR_HEIGHT).max(1.0),
            ),
        )?;

        log::info!("Created tab webview: {} in window: {}", tab_id, window_id);

        // Inject theme from config (not hardcoded)
        let theming = app.state::<Arc<dyn ThemingEngine<R>>>();
        let active_theme = theming.get_active_theme();
        theming.inject_theme(&webview, &active_theme);

        // Inject network monitor — intercepts fetch and XHR to log status/errors
        let network_monitor_js = r#"
            (function() {
                const log = (msg) => {
                    console.log(msg);
                    if (window.__TAURI__?.core?.invoke) {
                        // The log_network_event command in Rust has origin validation and truncation.
                        window.__TAURI__.core.invoke('log_network_event', { event: msg });
                    }
                };

                const getOrigin = (url) => {
                    try {
                        const u = new URL(url);
                        return `${u.protocol}//${u.hostname}`;
                    } catch {
                        return 'invalid-url';
                    }
                };

                // Monitor Fetch
                const originalFetch = window.fetch;
                window.fetch = async (...args) => {
                    const url = args[0] instanceof Request ? args[0].url : args[0];
                    const origin = getOrigin(url); // Log origin instead of full URL
                    try {
                        const response = await originalFetch(...args);
                        log(`FETCH SUCCESS: ${response.status} from ${origin}`);
                        return response;
                    } catch (error) {
                        log(`FETCH ERROR: ${origin} - ${error.message}`);
                        throw error;
                    }
                };

                // Monitor XHR
                const originalOpen = XMLHttpRequest.prototype.open;
                XMLHttpRequest.prototype.open = function(method, url) {
                    this._url = url;
                    const origin = getOrigin(url); // Log origin instead of full URL
                    this.addEventListener('load', function() {
                        log(`XHR SUCCESS: ${this.status} from ${origin}`);
                    });
                    this.addEventListener('error', function() {
                        log(`XHR ERROR: ${origin}`);
                    });
                    return originalOpen.apply(this, arguments);
                };
                log('Network monitoring active.');
            })();
        "#;
        let _ = webview.eval(network_monitor_js);

        Ok(Self {
            tab_id,
            window_id: window_id.to_string(),
            webview,
        })
    }

    pub fn load_url(&self, app: &AppHandle, url: &str) -> tauri::Result<()> {
        let policy = app.state::<Arc<dyn PolicyEnforcer>>();

        // Zero-Trust Enforcement: Validate URL before navigation
        if !policy.validate_url(url) {
            log::warn!("Zero-Trust Policy Blocked navigation to: {}", url);
            return Ok(()); // Fail closed (don't navigate)
        }

        log::info!("Tab {}: Loading URL {}", self.tab_id, url);
        let url = url
            .parse::<Url>()
            .map_err(|e| tauri::Error::AssetNotFound(e.to_string()))?;
        self.webview.navigate(url)?;
        Ok(())
    }

    pub fn show(&self) -> tauri::Result<()> {
        self.webview.show()?;
        Ok(())
    }

    pub fn hide(&self) -> tauri::Result<()> {
        self.webview.hide()?;
        Ok(())
    }

    pub fn destroy(&self) -> tauri::Result<()> {
        log::info!("Destroying tab: {}", self.tab_id);
        self.webview.close()?;
        Ok(())
    }
}

/// Routes secure popup requests into the application's internal TabManager.
/// Guaranteeing that any nested popups (e.g. nested OAuth flows) inherit
/// the exact same zero-trust `on_navigation` and `on_new_window` policies
/// as their parent window via the TabController factory.
pub fn spawn_secure_popup<R: Runtime>(
    app: &AppHandle<R>,
    _policy: Arc<dyn PolicyEnforcer>,
    url: Url,
) {
    log::info!(
        "Intercepted popup request. Routing into a secure in-app tab: {}",
        url.as_str()
    );

    // Instead of spawning a completely disconnected OS window, dispatch the popup
    // into our managed TabOrchestrator. This keeps the application bounded strictly
    // to a single window and enforces all Zero-Trust policies recursively since
    // create_tab() uses the TabController factory.
    if let Some(orchestrator) = app.try_state::<Arc<dyn crate::traits::TabOrchestrator<R>>>() {
        match orchestrator.inner().create_tab(app, "main", url.as_str()) {
            Ok(tab_id) => {
                if let Err(error) = orchestrator.inner().show_tab(&tab_id) {
                    log::error!("Failed to show popup tab: {}", error);
                }
            }
            Err(error) => {
                log::error!(
                    "Zero-Trust: Failed to route popup into managed tab: {}",
                    error
                )
            }
        }
    } else {
        log::error!("Zero-Trust: Cannot spawn tab securely. TabOrchestrator missing from state.");
    }
}

/// A centralized factory for creating WebviewBuilders with guaranteed security listeners.
/// Ensures all created webviews (tabs or popups) enforce on_navigation and on_new_window policies.
pub fn create_secure_webview_builder<R: Runtime>(
    app: &AppHandle<R>,
    label: &str,
    url: &Url,
    window_id: &str,
    policy: Arc<dyn PolicyEnforcer>,
) -> WebviewBuilder<R> {
    let webview_builder = WebviewBuilder::new(label, WebviewUrl::External(url.clone()))
        .initialization_script(tab_shortcuts_script(label, window_id));

    let nav_app = app.clone();
    let nav_policy = policy.clone();
    let popup_app = app.clone();
    let popup_policy = policy.clone();
    let window_id_owned = window_id.to_string();
    let title_app = app.clone();
    let title_tab_id = label.to_string();
    let load_app = app.clone();
    let load_tab_id = label.to_string();

    webview_builder
        .on_document_title_changed(move |webview, title| {
            if let Ok(url) = webview.url() {
                persist_tab_metadata(&title_app, &title_tab_id, Some(title), url.as_str());
            }
        })
        .on_page_load(move |_, payload| {
            persist_tab_metadata(&load_app, &load_tab_id, None, payload.url().as_str());
        })
        .on_navigation(move |url| {
            let window_id = &window_id_owned;
            let url_str = url.as_str();

            // Intercept custom window control actions
            if url_str.starts_with("lotion-action://") {
                let action = url_str.strip_prefix("lotion-action://").unwrap_or("");
                log::info!("Intercepted Lotion action: {}", action);

                if let Some(w) = nav_app.get_window(window_id) {
                    match action {
                        "window:close" => {
                            let _ = w.close();
                        }
                        "window:minimize" => {
                            let _ = w.minimize();
                        }
                        "window:maximize" => {
                            if let Ok(true) = w.is_maximized() {
                                let _ = w.unmaximize();
                            } else {
                                let _ = w.maximize();
                            }
                        }
                        "tab:new" => {
                            let notion_url = "https://www.notion.so";
                            if let Some(orchestrator) =
                                nav_app.try_state::<Arc<dyn crate::traits::TabOrchestrator<R>>>()
                            {
                                if let Ok(new_id) = orchestrator
                                    .inner()
                                    .create_tab(&nav_app, window_id, notion_url)
                                {
                                    let _ = orchestrator.inner().show_tab(&new_id);
                                }
                            }
                        }
                        _ => {
                            log::warn!("Unknown Lotion action: {}", action);
                        }
                    }
                }
                return false; // Prevent actual navigation
            }

            if nav_policy.validate_url(url_str) {
                true // Allow internal navigation to Notion
            } else if nav_policy.validate_external_link(url_str) {
                log::info!(
                    "Zero-Trust: Opening validated external link in default browser: {}",
                    url_str
                );
                use tauri_plugin_shell::ShellExt;
                #[allow(deprecated)]
                let _ = nav_app.shell().open(url_str.to_string(), None);
                false // Block navigation in the webview
            } else {
                log::warn!(
                    "Zero-Trust: BLOCKED unauthorized navigation attempt to: {}",
                    url_str
                );
                false // Block everything else
            }
        })
        .on_new_window(move |url, _| {
            use tauri_plugin_shell::ShellExt;
            let url_str = url.as_str();

            if popup_policy.should_route_popup_to_system_browser(url_str) {
                if popup_policy.validate_external_link(url_str) {
                    log::info!(
                        "Routing new window request (popup) to system browser: {}",
                        url_str
                    );
                    #[allow(deprecated)]
                    let _ = popup_app.shell().open(url_str.to_string(), None);
                } else {
                    log::warn!(
                        "Zero-Trust: BLOCKED unauthorized popup attempt to: {}",
                        url_str
                    );
                }
                tauri::webview::NewWindowResponse::Deny
            } else {
                // Spawn a controlled popup using the recursive secure factory
                spawn_secure_popup(&popup_app, popup_policy.clone(), url.clone());
                tauri::webview::NewWindowResponse::Deny
            }
        })
}

fn tab_shortcuts_script(tab_id: &str, window_id: &str) -> String {
    format!(
        r#"
document.addEventListener("keydown", async (event) => {{
  const invoke = window.__TAURI__?.core?.invoke;
  if (!invoke || !(event.ctrlKey || event.metaKey) || event.altKey) return;

  const key = event.key.toLowerCase();
  if (key === "t") {{
    event.preventDefault();
    await invoke("new_tab", {{ windowId: "{window_id}" }});
  }} else if (key === "w") {{
    event.preventDefault();
    await invoke("close_tab", {{ tabId: "{tab_id}" }});
  }} else if (event.key === "Tab") {{
    event.preventDefault();
    await invoke("switch_relative_tab", {{
      windowId: "{window_id}",
      direction: event.shiftKey ? -1 : 1
    }});
  }}
}}, true);
"#
    )
}

fn persist_tab_metadata<R: Runtime>(
    app: &AppHandle<R>,
    tab_id: &str,
    title: Option<String>,
    url: &str,
) {
    let is_notion_url = Url::parse(url)
        .ok()
        .filter(|url| url.scheme() == "https")
        .and_then(|url| url.host_str().map(str::to_owned))
        .is_some_and(|host| {
            host == "notion.so"
                || host.ends_with(".notion.so")
                || host == "notion.com"
                || host.ends_with(".notion.com")
        });
    if !is_notion_url {
        return;
    }

    let state = app.state::<Arc<tokio::sync::Mutex<crate::state::AppState>>>();
    let mut state = state.blocking_lock();
    let Some(tab) = state.tabs.get_mut(tab_id) else {
        return;
    };
    if let Some(title) = title.filter(|title| !title.trim().is_empty()) {
        tab.title = title;
    }
    tab.url = url.to_string();

    if let Some(secret) = app.try_state::<Arc<Vec<u8>>>() {
        if let Err(error) = state.save_to_disk(secret.inner().as_slice()) {
            log::error!("Failed to save tab metadata: {}", error);
        }
    }
}
