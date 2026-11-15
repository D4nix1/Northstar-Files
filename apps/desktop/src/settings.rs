use std::{fs, path::PathBuf};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ThemeChoice {
    Light,
    #[default]
    Dark,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ViewMode {
    #[default]
    List,
    Grid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub theme: ThemeChoice,
    pub view: ViewMode,
    pub font_scale: f32,
    pub compact_rows: bool,
    pub show_hidden: bool,
    pub show_file_extensions: bool,
    pub dual_pane: bool,
    pub confirm_delete: bool,
    pub favorites: Vec<PathBuf>,
    pub accent: [u8; 3],
    pub saved_filters: Vec<SavedFilter>,
    pub recent_paths: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedFilter {
    pub name: String,
    pub extension: String,
    pub min_size: String,
    pub max_size: String,
    pub created_after: String,
    pub created_before: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        let home = std::env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from));
        let mut favorites = Vec::new();
        if let Some(home) = home {
            for folder in ["Desktop", "Documents", "Downloads", "Pictures"] {
                let path = home.join(folder);
                if path.is_dir() {
                    favorites.push(path);
                }
            }
        }
        Self {
            theme: ThemeChoice::Dark,
            view: ViewMode::List,
            font_scale: 1.0,
            compact_rows: false,
            show_hidden: false,
            show_file_extensions: true,
            dual_pane: false,
            confirm_delete: true,
            favorites,
            accent: [105, 145, 255],
            saved_filters: Vec::new(),
            recent_paths: Vec::new(),
        }
    }
}

fn settings_path() -> Option<PathBuf> {
    ProjectDirs::from("org", "Northstar", "Northstar Files")
        .map(|dirs| dirs.config_dir().join("settings.json"))
}

pub fn load() -> AppSettings {
    settings_path()
        .and_then(|path| fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

pub fn save(settings: &AppSettings) -> Result<(), String> {
    let path = settings_path().ok_or("Não foi possível determinar a pasta de configurações")?;
    let parent = path.parent().ok_or("Caminho de configurações inválido")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let bytes = serde_json::to_vec_pretty(settings).map_err(|e| e.to_string())?;
    let temp = path.with_extension("json.tmp");
    fs::write(&temp, bytes).map_err(|e| e.to_string())?;
    fs::rename(temp, path).map_err(|e| e.to_string())
}
