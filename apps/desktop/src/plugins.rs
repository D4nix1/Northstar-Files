//! Small user-authored Rhai extensions. Scripts have no filesystem, network,
//! process, or OS bindings. Their return value is interpreted by a tiny,
//! allow-listed host API rather than executed as arbitrary commands.

use std::{fs, path::PathBuf};

use directories::ProjectDirs;
use rhai::{Engine, Scope};

#[derive(Debug, Clone)]
pub struct UserPlugin {
    pub name: String,
    pub file: PathBuf,
    pub source: String,
    pub enabled: bool,
}

#[derive(Default)]
pub struct PluginRegistry {
    pub plugins: Vec<UserPlugin>,
    pub selected: Option<usize>,
    pub draft_name: String,
    pub draft_source: String,
    pub message: String,
}

impl PluginRegistry {
    pub fn new() -> Self {
        let mut registry = Self::default();
        registry.draft_name = "Meu primeiro plugin".into();
        registry.draft_source = EXAMPLE_PLUGIN.into();
        registry.reload();
        registry
    }

    fn plugin_dir() -> Option<PathBuf> {
        ProjectDirs::from("org", "Northstar", "Northstar Files")
            .map(|dirs| dirs.data_local_dir().join("plugins"))
    }

    pub fn reload(&mut self) {
        let Some(dir) = Self::plugin_dir() else {
            self.message = "Pasta de plugins indisponível neste sistema".into();
            return;
        };
        if let Err(error) = fs::create_dir_all(&dir) {
            self.message = format!("Não foi possível abrir a pasta de plugins: {error}");
            return;
        }
        let mut found = Vec::new();
        if let Ok(items) = fs::read_dir(&dir) {
            for item in items.flatten() {
                let path = item.path();
                if path.extension().is_some_and(|ext| ext == "rhai") {
                    if let Ok(source) = fs::read_to_string(&path) {
                        let name = path
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("Plugin")
                            .replace('_', " ");
                        found.push(UserPlugin {
                            name,
                            file: path.clone(),
                            source,
                            enabled: !path.with_extension("disabled").exists(),
                        });
                    }
                }
            }
        }
        found.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        self.plugins = found;
        self.selected = self.selected.filter(|i| *i < self.plugins.len());
        self.message = format!("{} plugin(s) encontrado(s)", self.plugins.len());
    }

    pub fn save_draft(&mut self) {
        let Some(dir) = Self::plugin_dir() else {
            self.message = "Pasta de plugins indisponível".into();
            return;
        };
        let stem = safe_stem(&self.draft_name);
        if stem.is_empty() {
            self.message = "Dê um nome ao plugin".into();
            return;
        }
        let mut engine = Engine::new();
        engine.set_max_operations(20_000);
        if let Err(error) = engine.compile(&self.draft_source) {
            self.message = format!("Corrija o script antes de salvar: {error}");
            return;
        }
        if let Err(error) = fs::create_dir_all(&dir) {
            self.message = format!("Não foi possível criar a pasta: {error}");
            return;
        }
        let path = dir.join(format!("{stem}.rhai"));
        match fs::write(path, &self.draft_source) {
            Ok(()) => {
                self.reload();
                self.message = format!("Plugin ‘{}’ salvo", self.draft_name);
            }
            Err(error) => self.message = format!("Falha ao salvar plugin: {error}"),
        }
    }

    pub fn select(&mut self, index: usize) {
        if let Some(plugin) = self.plugins.get(index) {
            self.draft_name = plugin.name.clone();
            self.draft_source = plugin.source.clone();
            self.selected = Some(index);
        }
    }

    pub fn set_enabled(&mut self, index: usize, enabled: bool) {
        let Some(plugin) = self.plugins.get_mut(index) else {
            return;
        };
        let marker = plugin.file.with_extension("disabled");
        let result = if enabled {
            fs::remove_file(marker).or_else(|error| {
                if error.kind() == std::io::ErrorKind::NotFound {
                    Ok(())
                } else {
                    Err(error)
                }
            })
        } else {
            fs::write(marker, b"disabled")
        };
        match result {
            Ok(()) => {
                plugin.enabled = enabled;
                self.message = if enabled {
                    "Plugin ativado"
                } else {
                    "Plugin desativado"
                }
                .into();
            }
            Err(error) => {
                self.message = format!("Não foi possível salvar o estado do plugin: {error}")
            }
        }
    }

    pub fn run_selected(&self, name: &str, path: &str, is_dir: bool) -> Result<String, String> {
        let Some(index) = self.selected else {
            return Err("Escolha um plugin na aba Plugins".into());
        };
        self.run(index, name, path, is_dir)
    }

    pub fn run(
        &self,
        index: usize,
        name: &str,
        path: &str,
        is_dir: bool,
    ) -> Result<String, String> {
        let Some(plugin) = self.plugins.get(index) else {
            return Err("Plugin não encontrado".into());
        };
        if !plugin.enabled {
            return Err("Este plugin está desativado".into());
        }
        let mut engine = Engine::new();
        engine.set_max_operations(20_000);
        let ast = engine.compile(&plugin.source).map_err(|e| e.to_string())?;
        let mut scope = Scope::new();
        engine
            .call_fn::<String>(
                &mut scope,
                &ast,
                "run",
                (name.to_owned(), path.to_owned(), is_dir),
            )
            .map_err(|e| format!("O plugin precisa implementar run(nome, caminho, pasta): {e}"))
    }

    pub fn delete_selected(&mut self) {
        let Some(index) = self.selected else {
            return;
        };
        let Some(plugin) = self.plugins.get(index) else {
            return;
        };
        let path = plugin.file.clone();
        let marker = plugin.file.with_extension("disabled");
        match fs::remove_file(path) {
            Ok(()) => {
                let _ = fs::remove_file(marker);
                self.selected = None;
                self.reload();
                self.message = "Plugin removido".into();
            }
            Err(error) => self.message = format!("Falha ao remover plugin: {error}"),
        }
    }
}

fn safe_stem(name: &str) -> String {
    name.trim()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .to_lowercase()
}

pub const EXAMPLE_PLUGIN: &str = r#"// Exemplo: aplique uma ação só a imagens PNG.
// A função recebe nome, caminho e se o item é uma pasta.
// Retorne um comando seguro: copy_path, favorite, open, toggle_hidden,
// theme:light|dark|system, view:list|grid, font:85..150,
// accent:#RRGGBB ou uma mensagem notify:.
fn run(name, path, is_dir) {
    if !is_dir && name.ends_with(".png") {
        "notify:Imagem PNG selecionada"
    } else {
        "notify:Este plugin foi executado"
    }
}
"#;

pub const CUSTOMIZATION_PLUGIN: &str = r#"// Exemplo: use uma fonte maior ao selecionar arquivos PDF.
fn run(name, path, is_dir) {
    if !is_dir && name.ends_with(".pdf") {
        "font:125"
    } else {
        "notify:Este item não precisa de um ajuste especial"
    }
}
"#;
