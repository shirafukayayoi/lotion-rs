use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm,
    Key,
    Nonce, // Or `Nonce` from `aes_gcm::aead::AeadCore`
};
use base64::{engine::general_purpose, Engine as _};
use pbkdf2::pbkdf2_hmac;
use rand::rngs::StdRng;
use rand_core::{RngCore, SeedableRng};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::collections::HashMap;
use std::path::PathBuf;

// const APPLICATION_SECRET: &[u8] = b"lotion-rs-super-secret-key-that-is-long-and-random-for-pbkdf2";
const PBKDF2_ITERATIONS: u32 = 100_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedState {
    pub data: String,  // Base64 encoded encrypted data
    pub nonce: String, // Base64 encoded nonce
}

fn get_encryption_key(app_secret: &[u8]) -> Key<Aes256Gcm> {
    // Use a stable, but unique per-machine, salt for PBKDF2.
    // Combining the application name and config directory path provides this.
    let salt_source = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("lotion-rs")
        .to_string_lossy()
        .into_owned();

    let mut key_bytes = Key::<Aes256Gcm>::default();
    pbkdf2_hmac::<Sha256>(
        app_secret,
        salt_source.as_bytes(),
        PBKDF2_ITERATIONS,
        &mut key_bytes,
    );
    key_bytes
}

fn encrypt_data(data: &[u8], key: &Key<Aes256Gcm>) -> Result<(Vec<u8>, Vec<u8>), String> {
    let cipher = Aes256Gcm::new(key);
    let mut rng = StdRng::from_entropy();
    let mut nonce_bytes = vec![0u8; 12]; // GCM nonces are 12 bytes
    rng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    cipher
        .encrypt(nonce, data)
        .map(|cipher_text| (cipher_text, nonce_bytes))
        .map_err(|e| format!("Encryption error: {:?}", e))
}

fn decrypt_data(
    encrypted_data: &[u8],
    nonce_bytes: &[u8],
    key: &Key<Aes256Gcm>,
) -> Result<Vec<u8>, String> {
    let cipher = Aes256Gcm::new(key);
    let nonce = Nonce::from_slice(nonce_bytes);

    cipher
        .decrypt(nonce, encrypted_data)
        .map_err(|e| format!("Decryption error: {:?}", e))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bounds {
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TabState {
    pub id: String,
    pub title: String,
    pub url: String,
    pub is_active: bool,
    pub is_pinned: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowState {
    pub id: String,
    pub bounds: Bounds,
    pub is_focused: bool,
    pub is_maximized: bool,
    pub is_minimized: bool,
    pub is_full_screen: bool,
    pub tab_ids: Vec<String>,
    pub active_tab_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppState {
    pub windows: HashMap<String, WindowState>,
    pub tabs: HashMap<String, TabState>,
    pub focused_window_id: Option<String>,
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

impl AppState {
    pub fn new() -> Self {
        Self {
            windows: HashMap::new(),
            tabs: HashMap::new(),
            focused_window_id: None,
        }
    }

    pub fn register_tab(&mut self, window_id: &str, mut tab: TabState) {
        let Some(window) = self.windows.get_mut(window_id) else {
            return;
        };

        for id in &window.tab_ids {
            if let Some(existing) = self.tabs.get_mut(id) {
                existing.is_active = false;
            }
        }

        tab.is_active = true;
        if !window.tab_ids.contains(&tab.id) {
            window.tab_ids.push(tab.id.clone());
        }
        window.active_tab_id = Some(tab.id.clone());
        self.tabs.insert(tab.id.clone(), tab);
    }

    pub fn activate_tab(&mut self, tab_id: &str) -> bool {
        let Some(window_id) = self
            .windows
            .iter()
            .find(|(_, window)| window.tab_ids.iter().any(|id| id == tab_id))
            .map(|(id, _)| id.clone())
        else {
            return false;
        };

        let tab_ids = self.windows[&window_id].tab_ids.clone();
        for id in tab_ids {
            if let Some(tab) = self.tabs.get_mut(&id) {
                tab.is_active = id == tab_id;
            }
        }
        self.windows
            .get_mut(&window_id)
            .expect("window disappeared while activating tab")
            .active_tab_id = Some(tab_id.to_string());
        true
    }

    pub fn remove_tab(&mut self, tab_id: &str) -> Option<(String, Option<String>)> {
        let (window_id, index) = self.windows.iter().find_map(|(window_id, window)| {
            window
                .tab_ids
                .iter()
                .position(|id| id == tab_id)
                .map(|index| (window_id.clone(), index))
        })?;

        self.tabs.remove(tab_id);
        let window = self.windows.get_mut(&window_id)?;
        let was_active = window.active_tab_id.as_deref() == Some(tab_id);
        window.tab_ids.remove(index);
        let next_tab_id = if !was_active {
            window.active_tab_id.clone()
        } else if window.tab_ids.is_empty() {
            None
        } else {
            Some(window.tab_ids[index.min(window.tab_ids.len() - 1)].clone())
        };
        window.active_tab_id = next_tab_id.clone();

        for id in &window.tab_ids {
            if let Some(tab) = self.tabs.get_mut(id) {
                tab.is_active = next_tab_id.as_ref() == Some(id);
            }
        }

        Some((window_id, next_tab_id))
    }

    pub fn update_tab(&mut self, tab_id: &str, title: String, url: String) {
        if let Some(tab) = self.tabs.get_mut(tab_id) {
            tab.title = title;
            tab.url = url;
        }
    }

    /// Returns the state file path (~/.config/lotion-rs/state.json)
    fn state_path() -> std::path::PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("lotion-rs")
            .join("state.json")
    }

    /// Save application state to disk.
    pub fn save_to_disk(&self, app_secret: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
        let path = Self::state_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let key = get_encryption_key(app_secret);
        let plaintext = serde_json::to_string(self)?;
        let (ciphertext, nonce) =
            encrypt_data(plaintext.as_bytes(), &key).map_err(Box::<dyn std::error::Error>::from)?;

        let encrypted_state = EncryptedState {
            data: general_purpose::STANDARD.encode(&ciphertext),
            nonce: general_purpose::STANDARD.encode(&nonce),
        };
        let json = serde_json::to_string_pretty(&encrypted_state)?;

        std::fs::write(&path, json)?;
        log::info!("Encrypted AppState saved to {}", path.display());
        Ok(())
    }

    /// Load application state from disk.
    pub fn load_from_disk(app_secret: &[u8]) -> Option<Self> {
        let path = Self::state_path();
        if path.exists() {
            let key = get_encryption_key(app_secret);
            match std::fs::read_to_string(&path) {
                Ok(contents) => {
                    // Try to load as encrypted state first
                    if let Ok(encrypted_state) = serde_json::from_str::<EncryptedState>(&contents) {
                        let decoded_data =
                            match general_purpose::STANDARD.decode(&encrypted_state.data) {
                                Ok(d) => d,
                                Err(e) => {
                                    log::warn!("Failed to base64 decode encrypted data: {}", e);
                                    return None;
                                }
                            };
                        let decoded_nonce =
                            match general_purpose::STANDARD.decode(&encrypted_state.nonce) {
                                Ok(n) => n,
                                Err(e) => {
                                    log::warn!("Failed to base64 decode nonce: {}", e);
                                    return None;
                                }
                            };

                        match decrypt_data(&decoded_data, &decoded_nonce, &key) {
                            Ok(plaintext_bytes) => match String::from_utf8(plaintext_bytes) {
                                Ok(plaintext) => match serde_json::from_str::<AppState>(&plaintext)
                                {
                                    Ok(state) => {
                                        log::info!(
                                            "Encrypted AppState loaded from {}",
                                            path.display()
                                        );
                                        return Some(state);
                                    }
                                    Err(e) => log::warn!("Failed to parse decrypted state: {}", e),
                                },
                                Err(e) => {
                                    log::warn!("Failed to convert decrypted bytes to string: {}", e)
                                }
                            },
                            Err(e) => log::warn!("Failed to decrypt state file: {}", e),
                        }
                    }

                    // If encrypted load failed, try to load as plaintext (for backward compatibility)
                    // This branch will be executed if deserialization to EncryptedState fails,
                    // which means it's likely an old unencrypted file.
                    if let Ok(state) = serde_json::from_str::<AppState>(&contents) {
                        log::warn!(
                            "Loaded unencrypted AppState from {}. Please re-save to encrypt.",
                            path.display()
                        );
                        return Some(state);
                    }
                    log::warn!("Failed to load state file as either encrypted or plaintext.");
                }
                Err(e) => log::warn!("Failed to read state file: {}", e),
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state_with_window() -> AppState {
        let mut state = AppState::new();
        state.windows.insert(
            "main".into(),
            WindowState {
                id: "main".into(),
                bounds: Bounds {
                    x: None,
                    y: None,
                    width: 1200.0,
                    height: 800.0,
                },
                is_focused: true,
                is_maximized: false,
                is_minimized: false,
                is_full_screen: false,
                tab_ids: Vec::new(),
                active_tab_id: None,
            },
        );
        state
    }

    fn tab(id: &str) -> TabState {
        TabState {
            id: id.into(),
            title: "Notion".into(),
            url: "https://www.notion.so".into(),
            is_active: false,
            is_pinned: false,
        }
    }

    #[test]
    fn tab_lifecycle_keeps_one_active_tab() {
        let mut state = state_with_window();
        state.register_tab("main", tab("first"));
        state.register_tab("main", tab("second"));
        state.register_tab("main", tab("third"));

        assert_eq!(
            state.windows["main"].active_tab_id.as_deref(),
            Some("third")
        );
        assert!(state.tabs["third"].is_active);
        assert!(!state.tabs["first"].is_active);

        assert!(state.activate_tab("second"));
        assert_eq!(
            state.windows["main"].active_tab_id.as_deref(),
            Some("second")
        );
        assert!(state.tabs["second"].is_active);

        let next = state.remove_tab("second");
        assert_eq!(next, Some(("main".into(), Some("third".into()))));
        assert_eq!(
            state.windows["main"].active_tab_id.as_deref(),
            Some("third")
        );
        assert!(state.tabs["third"].is_active);
    }

    #[test]
    fn removing_inactive_tab_preserves_active_tab_and_updates_metadata() {
        let mut state = state_with_window();
        state.register_tab("main", tab("first"));
        state.register_tab("main", tab("second"));
        state.update_tab(
            "second",
            "Help".into(),
            "https://www.notion.com/help".into(),
        );

        let next = state.remove_tab("first");
        assert_eq!(next, Some(("main".into(), Some("second".into()))));
        assert_eq!(state.tabs["second"].title, "Help");
        assert_eq!(state.tabs["second"].url, "https://www.notion.com/help");
        assert!(state.tabs["second"].is_active);
    }
}
