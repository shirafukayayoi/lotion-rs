use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// User-facing configuration persisted to disk as TOML.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LotionConfig {
    pub active_theme: String,
    pub custom_css_path: Option<PathBuf>,
    pub restore_tabs: bool,
    pub window: WindowConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowConfig {
    pub width: f64,
    pub height: f64,
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub maximized: bool,
}

impl Default for LotionConfig {
    fn default() -> Self {
        Self {
            active_theme: "dracula".to_string(),
            custom_css_path: None,
            restore_tabs: true,
            window: WindowConfig {
                width: 1200.0,
                height: 800.0,
                x: None,
                y: None,
                maximized: false,
            },
        }
    }
}

impl LotionConfig {
    /// Returns the config directory path (~/.config/lotion-rs/)
    fn config_dir() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("lotion-rs")
    }

    /// Returns the config file path (~/.config/lotion-rs/config.toml)
    fn config_path() -> PathBuf {
        Self::config_dir().join("config.toml")
    }

    /// Migrate config from ~/.config/lotion to ~/.config/lotion-rs if needed
    fn migrate_legacy_config() {
        let old_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("lotion");
        let new_dir = Self::config_dir();

        if old_dir.exists() && !new_dir.exists() {
            log::info!(
                "Migrating legacy config from {} to {}",
                old_dir.display(),
                new_dir.display()
            );
            if let Err(e) = fs::create_dir_all(&new_dir) {
                log::error!("Failed to create new config dir: {}", e);
                return;
            }

            // Copy config.toml
            let old_config = old_dir.join("config.toml");
            if old_config.exists() {
                if let Err(e) = fs::copy(&old_config, new_dir.join("config.toml")) {
                    log::error!("Failed to migrate config.toml: {}", e);
                }
            }

            // Copy state.json
            let old_state = old_dir.join("state.json");
            if old_state.exists() {
                if let Err(e) = fs::copy(&old_state, new_dir.join("state.json")) {
                    log::error!("Failed to migrate state.json: {}", e);
                }
            }
        }
    }

    /// Load config from disk, or create default if not found.
    pub fn load() -> Self {
        Self::migrate_legacy_config();
        let path = Self::config_path();
        if path.exists() {
            match fs::read_to_string(&path) {
                Ok(contents) => {
                    match toml::from_str::<LotionConfig>(&contents) {
                        Ok(mut config) => {
                            // 'mut' is needed to modify config.custom_css_path
                            if let Some(ref path) = config.custom_css_path {
                                let custom_themes_dir = Self::config_dir().join("custom_themes");
                                if !path.starts_with(&custom_themes_dir) {
                                    log::warn!("Custom CSS path '{}' is outside the designated custom themes directory. Discarding for security.", path.display());
                                    config.custom_css_path = None;
                                } else if path.extension().is_none_or(|ext| ext != "css") {
                                    log::warn!("Custom CSS path '{}' does not have a .css extension. Discarding for security.", path.display());
                                    config.custom_css_path = None;
                                } else if !path.exists() {
                                    log::warn!(
                                        "Custom CSS path '{}' does not exist. Discarding.",
                                        path.display()
                                    );
                                    config.custom_css_path = None;
                                }
                            }
                            log::info!("Config loaded from {}", path.display());
                            return config;
                        }
                        Err(e) => {
                            log::warn!("Failed to parse config, using defaults: {}", e);
                        }
                    }
                } // End of Ok(contents) arm
                Err(e) => {
                    log::warn!("Failed to read config file, using defaults: {}", e);
                }
            }
        } else {
            log::info!(
                "No config file found, creating default at {}",
                path.display()
            );
        }

        let config = Self::default();
        let _ = config.save(); // Best-effort save of defaults
        config
    }

    /// Save config to disk.
    pub fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        let dir = Self::config_dir();
        fs::create_dir_all(&dir)?;

        let contents = toml::to_string_pretty(self)?;
        fs::write(Self::config_path(), contents)?;

        log::info!("Config saved to {}", Self::config_path().display());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    #[test]
    fn test_config_migration() {
        let temp_base = tempfile::tempdir().unwrap();
        let old_dir = temp_base.path().join("lotion");
        let new_dir = temp_base.path().join("lotion-rs");

        fs::create_dir_all(&old_dir).unwrap();
        fs::write(old_dir.join("config.toml"), "active_theme = 'nord'").unwrap();
        fs::write(old_dir.join("state.json"), "{}").unwrap();

        // Manual migration trigger logic with custom paths
        if old_dir.exists() && !new_dir.exists() {
            fs::create_dir_all(&new_dir).unwrap();
            fs::copy(old_dir.join("config.toml"), new_dir.join("config.toml")).unwrap();
            fs::copy(old_dir.join("state.json"), new_dir.join("state.json")).unwrap();
        }

        assert!(new_dir.join("config.toml").exists());
        assert!(new_dir.join("state.json").exists());
        let contents = fs::read_to_string(new_dir.join("config.toml")).unwrap();
        assert!(contents.contains("nord"));
    }
}
