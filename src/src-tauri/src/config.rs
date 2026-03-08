use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub origin: String,
    pub session_path: Option<String>,
    #[serde(default)]
    pub download_dir: Option<String>,
    #[serde(default = "default_thread_count")]
    pub thread_count: u32,
}

fn default_thread_count() -> u32 {
    16
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            origin: "https://rezka.ag".to_string(),
            session_path: None,
            download_dir: None,
            thread_count: 16,
        }
    }
}

impl AppConfig {
    fn config_path(app_data_dir: &PathBuf) -> PathBuf {
        app_data_dir.join("config.json")
    }

    pub fn load(app_data_dir: &PathBuf) -> Self {
        let path = Self::config_path(app_data_dir);
        if path.exists() {
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(config) = serde_json::from_str(&content) {
                    return config;
                }
            }
        }
        Self::default()
    }

    pub fn save(&self, app_data_dir: &PathBuf) -> Result<(), String> {
        let path = Self::config_path(app_data_dir);
        fs::create_dir_all(app_data_dir).map_err(|e| e.to_string())?;
        let content = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        fs::write(&path, content).map_err(|e| e.to_string())
    }

    pub fn session_file_path(&self, app_data_dir: &PathBuf) -> PathBuf {
        if let Some(ref custom_path) = self.session_path {
            PathBuf::from(custom_path)
        } else {
            app_data_dir.join("session.json")
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionData {
    pub cookies: HashMap<String, String>,
    pub origin: String,
}

impl SessionData {
    pub fn load(path: &PathBuf) -> Option<Self> {
        if path.exists() {
            if let Ok(content) = fs::read_to_string(path) {
                return serde_json::from_str(&content).ok();
            }
        }
        None
    }

    pub fn save(&self, path: &PathBuf) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let content = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        fs::write(path, content).map_err(|e| e.to_string())
    }

    pub fn delete(path: &PathBuf) -> Result<(), String> {
        if path.exists() {
            fs::remove_file(path).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}
