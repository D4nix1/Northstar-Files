#![forbid(unsafe_code)]
#![cfg_attr(windows, windows_subsystem = "windows")]

mod plugins;
mod settings;
mod uninstaller;

mod localization;

use std::{
    path::{Path, PathBuf},
    sync::{
        OnceLock,
        mpsc::{self, Receiver, Sender},
    },
    time::Duration,
};

use chrono::{DateTime, Local};
use eframe::egui;
use file_engine::{DirectoryEntry, copy_path, list_directory, rename_path};
use settings::{AppSettings, ThemeChoice, ViewMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Screen {
    Home,
    Files,
    Tools,
    Customize,
    Plugins,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum ToolPage {
    #[default]
    FolderSizes,
    Duplicates,
    Compare,
    ContentSearch,
    BatchRename,
    Uninstaller,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IconKind {
    Home,
    Folder,
    Download,
    Document,
    Image,
    Audio,
    Video,
    Trash,
    File,
    App,
    Archive,
    Code,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SortBy {
    Name,
    Size,
    Created,
}

#[derive(Clone)]
struct Tab {
    path: PathBuf,
    back: Vec<PathBuf>,
    forward: Vec<PathBuf>,
    entries: Vec<DirectoryEntry>,
    loading: bool,
    error: Option<String>,
    selected: Vec<PathBuf>,
}

impl Tab {
    fn at(path: PathBuf) -> Self {
        Self {
            path,
            back: vec![],
            forward: vec![],
            entries: vec![],
            loading: true,
            error: None,
            selected: vec![],
        }
    }
}

enum Event {
    Listed {
        pane: usize,
        tab: usize,
        path: PathBuf,
        result: Result<Vec<DirectoryEntry>, String>,
    },
    Operation {
        description: String,
        result: Result<(), String>,
    },
    FolderSizes {
        path: PathBuf,
        result: Result<Vec<(PathBuf, u64)>, String>,
    },
    Duplicates {
        path: PathBuf,
        result: Result<Vec<file_engine::DuplicateGroup>, String>,
    },
    Compared {
        left: PathBuf,
        right: PathBuf,
        result: Result<Vec<file_engine::ComparedItem>, String>,
    },
    ContentSearch {
        path: PathBuf,
        result: Result<Vec<PathBuf>, String>,
    },
}

#[derive(Clone)]
struct Transfer {
    sources: Vec<PathBuf>,
    cut: bool,
}

#[derive(Clone)]
enum Dialog {
    Delete(Vec<PathBuf>),
    Rename(PathBuf, usize),
    NewFolder(usize),
    Transfer(Vec<(PathBuf, PathBuf)>, bool),
    Uninstall(uninstaller::InstalledProgram),
}

#[derive(Clone)]
enum UiAction {
    Open(PathBuf, usize),
    Favorite(PathBuf),
    CopyPath(PathBuf),
    Rename(PathBuf, usize),
    Delete(PathBuf),
    DeleteMany(Vec<PathBuf>),
    NewFolder(usize),
    Copy(PathBuf, bool),
    CopyMany(Vec<PathBuf>, bool),
    Paste,
    RunPlugin(PathBuf, usize, usize),
}

struct ExplorerApp {
    tabs: Vec<Tab>,
    active_tab: usize,
    active_pane: usize,
    right: Tab,
    settings: AppSettings,
    plugins: plugins::PluginRegistry,
    screen: Screen,
    tool_page: ToolPage,
    tool_root: PathBuf,
    folder_sizes: std::collections::HashMap<PathBuf, u64>,
    folder_sizes_root: Option<PathBuf>,
    scanning_folders: bool,
    duplicate_groups: Vec<file_engine::DuplicateGroup>,
    scanning_duplicates: bool,
    tool_message: String,
    installed_programs: Vec<uninstaller::InstalledProgram>,
    uninstall_filter: String,
    compare_left: PathBuf,
    compare_right: PathBuf,
    compare_results: Vec<file_engine::ComparedItem>,
    scanning_compare: bool,
    content_query: String,
    content_results: Vec<PathBuf>,
    searching_content: bool,
    rename_prefix: String,
    rename_suffix: String,
    batch_preview: Vec<(PathBuf, String)>,
    dual_pane: bool,
    search: String,
    extension_filter: String,
    min_size_filter: String,
    max_size_filter: String,
    created_after_filter: String,
    created_before_filter: String,
    saved_filter_name: String,
    path_input: String,
    sender: Sender<Event>,
    receiver: Receiver<Event>,
    runtime: Option<tokio::runtime::Runtime>,
    status: String,
    sort_by: SortBy,
    transfer: Option<Transfer>,
    dialog: Option<Dialog>,
    rename_text: String,
    folder_text: String,
    show_help: bool,
}

impl ExplorerApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let settings = settings::load();
        apply_theme(&cc.egui_ctx, &settings);
        let (sender, receiver) = mpsc::channel();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .ok();
        let home = std::env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("C:\\"));
        let start = directories::UserDirs::new()
            .and_then(|dirs| dirs.desktop_dir().map(Path::to_path_buf))
            .filter(|path| path.is_dir())
            .unwrap_or_else(|| home.clone());
        let mut app = Self {
            tabs: vec![Tab::at(start.clone())],
            active_tab: 0,
            active_pane: 0,
            right: Tab::at(home.clone()),
            dual_pane: settings.dual_pane,
            settings,
            plugins: plugins::PluginRegistry::new(),
            screen: Screen::Home,
            tool_page: ToolPage::FolderSizes,
            tool_root: start.clone(),
            folder_sizes: std::collections::HashMap::new(),
            folder_sizes_root: None,
            scanning_folders: false,
            duplicate_groups: Vec::new(),
            scanning_duplicates: false,
            tool_message: String::new(),
            installed_programs: Vec::new(),
            uninstall_filter: String::new(),
            compare_left: start.clone(),
            compare_right: home.clone(),
            compare_results: Vec::new(),
            scanning_compare: false,
            content_query: String::new(),
            content_results: Vec::new(),
            searching_content: false,
            rename_prefix: String::new(),
            rename_suffix: String::new(),
            batch_preview: Vec::new(),
            search: String::new(),
            extension_filter: String::new(),
            min_size_filter: String::new(),
            max_size_filter: String::new(),
            created_after_filter: String::new(),
            created_before_filter: String::new(),
            saved_filter_name: String::new(),
            path_input: start.display().to_string(),
            sender,
            receiver,
            runtime,
            status: "Bem-vindo! Escolha uma pasta para começar.".into(),
            sort_by: SortBy::Name,
            transfer: None,
            dialog: None,
            rename_text: String::new(),
            folder_text: String::new(),
            show_help: false,
        };
        app.load(0);
        if app.dual_pane {
            app.load(1);
        }
        app
    }

    fn current(&self) -> &Tab {
        &self.tabs[self.active_tab]
    }
    fn current_mut(&mut self) -> &mut Tab {
        &mut self.tabs[self.active_tab]
    }
    fn active_directory(&self) -> &Tab {
        if self.active_pane == 0 {
            self.current()
        } else {
            &self.right
        }
    }
    fn active_directory_mut(&mut self) -> &mut Tab {
        if self.active_pane == 0 {
            self.current_mut()
        } else {
            &mut self.right
        }
    }

    fn entry_matches(&self, entry: &DirectoryEntry, name_query: &str) -> bool {
        if !name_query.is_empty() && !entry.name.to_lowercase().contains(name_query) {
            return false;
        }
        let extension = self
            .extension_filter
            .trim()
            .trim_start_matches('.')
            .to_lowercase();
        if !extension.is_empty()
            && entry
                .path
                .extension()
                .and_then(|ext| ext.to_str())
                .unwrap_or("")
                .to_lowercase()
                != extension
        {
            return false;
        }
        let folder_size = (self.folder_sizes_root.as_deref() == entry.path.parent())
            .then(|| self.folder_sizes.get(&entry.path).copied())
            .flatten();
        let size = entry.size.or(folder_size);
        if let Some(minimum) = self.min_size_filter.trim().parse::<u64>().ok() {
            if size.is_none_or(|size| size < minimum) {
                return false;
            }
        }
        if let Some(maximum) = self.max_size_filter.trim().parse::<u64>().ok() {
            if size.is_none_or(|size| size > maximum) {
                return false;
            }
        }
        let created = entry
            .created
            .map(|time| DateTime::<Local>::from(time).format("%Y-%m-%d").to_string());
        let after = self.created_after_filter.trim();
        if !after.is_empty() && created.as_deref().is_none_or(|date| date < after) {
            return false;
        }
        let before = self.created_before_filter.trim();
        if !before.is_empty() && created.as_deref().is_none_or(|date| date > before) {
            return false;
        }
        true
    }

    fn persist(&mut self) {
        if let Err(error) = settings::save(&self.settings) {
            self.status = format!("Não foi possível salvar as preferências: {error}");
        }
    }

    fn load(&mut self, pane: usize) {
        let path = if pane == 0 {
            self.current().path.clone()
        } else {
            self.right.path.clone()
        };
        let tab_index = self.active_tab;
        let tab = if pane == 0 {
            self.current_mut()
        } else {
            &mut self.right
        };
        tab.loading = true;
        tab.error = None;
        if let Some(runtime) = &self.runtime {
            let tx = self.sender.clone();
            runtime.spawn(async move {
                let result = list_directory(&path).await.map_err(|e| e.to_string());
                let _ = tx.send(Event::Listed {
                    pane,
                    tab: tab_index,
                    path,
                    result,
                });
            });
        } else {
            let tab = if pane == 0 {
                self.current_mut()
            } else {
                &mut self.right
            };
            tab.loading = false;
            tab.error = Some("Não foi possível iniciar as tarefas do aplicativo".into());
        }
    }

    fn navigate(&mut self, pane: usize, path: PathBuf) {
        self.active_pane = pane;
        let tab = if pane == 0 {
            self.current_mut()
        } else {
            &mut self.right
        };
        if tab.path != path {
            tab.back.push(tab.path.clone());
            tab.forward.clear();
            tab.path = path.clone();
            tab.selected.clear();
        }
        if pane == 0 {
            self.path_input = self.current().path.display().to_string();
            if path.is_dir() {
                self.settings.recent_paths.retain(|recent| recent != &path);
                self.settings.recent_paths.insert(0, path.clone());
                self.settings.recent_paths.truncate(8);
                self.persist();
            }
        }
        self.load(pane);
    }

    fn travel(&mut self, forward: bool) {
        let tab = self.current_mut();
        let path = if forward {
            tab.forward.pop()
        } else {
            tab.back.pop()
        };
        if let Some(path) = path {
            if forward {
                tab.back.push(tab.path.clone());
            } else {
                tab.forward.push(tab.path.clone());
            }
            tab.path = path;
            tab.selected.clear();
            self.path_input = self.current().path.display().to_string();
            self.load(0);
        }
    }

    fn poll(&mut self) {
        while let Ok(event) = self.receiver.try_recv() {
            match event {
                Event::Listed {
                    pane,
                    tab,
                    path,
                    result,
                } => {
                    let target = if pane == 0 {
                        let Some(target) = self.tabs.get_mut(tab) else {
                            continue;
                        };
                        if target.path != path {
                            continue;
                        }
                        target
                    } else {
                        if self.right.path != path {
                            continue;
                        }
                        &mut self.right
                    };
                    target.loading = false;
                    match result {
                        Ok(entries) => {
                            target.entries = entries;
                            target.error = None;
                        }
                        Err(error) => target.error = Some(error),
                    }
                }
                Event::Operation {
                    description,
                    result,
                } => {
                    self.status = match result {
                        Ok(()) => description,
                        Err(e) => format!("{description}: {e}"),
                    };
                    self.load(0);
                    if self.dual_pane {
                        self.load(1);
                    }
                }
                Event::FolderSizes { path, result } => {
                    self.scanning_folders = false;
                    match result {
                        Ok(sizes) => {
                            self.folder_sizes = sizes.into_iter().collect();
                            self.folder_sizes_root = Some(path.clone());
                            self.tool_message = format!(
                                "Tamanhos calculados para {} itens",
                                self.folder_sizes.len()
                            );
                        }
                        Err(error) => self.tool_message = error,
                    }
                }
                Event::Duplicates { path, result } => {
                    if path != self.tool_root {
                        continue;
                    }
                    self.scanning_duplicates = false;
                    match result {
                        Ok(groups) => {
                            self.tool_message = if groups.is_empty() {
                                "Nenhum duplicado encontrado".into()
                            } else {
                                format!("{} grupos de duplicados encontrados", groups.len())
                            };
                            self.duplicate_groups = groups;
                        }
                        Err(error) => self.tool_message = error,
                    }
                }
                Event::Compared {
                    left,
                    right,
                    result,
                } => {
                    if left != self.compare_left || right != self.compare_right {
                        continue;
                    }
                    self.scanning_compare = false;
                    match result {
                        Ok(items) => {
                            self.tool_message = format!("{} diferenças encontradas", items.len());
                            self.compare_results = items;
                        }
                        Err(error) => self.tool_message = error,
                    }
                }
                Event::ContentSearch { path, result } => {
                    if path != self.tool_root {
                        continue;
                    }
                    self.searching_content = false;
                    match result {
                        Ok(paths) => {
                            self.tool_message =
                                format!("{} arquivos contêm esse texto", paths.len());
                            self.content_results = paths;
                        }
                        Err(error) => self.tool_message = error,
                    }
                }
            }
        }
    }

    fn scan_folder_sizes(&mut self) {
        let path = self.tool_root.clone();
        let tx = self.sender.clone();
        self.scanning_folders = true;
        self.tool_message = "Analisando tamanhos sem bloquear a interface…".into();
        if let Some(runtime) = &self.runtime {
            runtime.spawn(async move {
                let worker_path = path.clone();
                let result = tokio::task::spawn_blocking(move || {
                    file_engine::calculate_child_folder_sizes(worker_path)
                })
                .await
                .map_err(|error| error.to_string())
                .and_then(|result| result);
                let _ = tx.send(Event::FolderSizes { path, result });
            });
        } else {
            self.scanning_folders = false;
            self.tool_message = "Não foi possível iniciar as tarefas do aplicativo".into();
        }
    }

    fn scan_duplicates(&mut self) {
        let path = self.tool_root.clone();
        let tx = self.sender.clone();
        self.scanning_duplicates = true;
        self.duplicate_groups.clear();
        self.tool_message = "Comparando conteúdo dos arquivos…".into();
        if let Some(runtime) = &self.runtime {
            runtime.spawn(async move {
                let worker_path = path.clone();
                let result =
                    tokio::task::spawn_blocking(move || file_engine::find_duplicates(worker_path))
                        .await
                        .map_err(|error| error.to_string())
                        .and_then(|result| result);
                let _ = tx.send(Event::Duplicates { path, result });
            });
        } else {
            self.scanning_duplicates = false;
            self.tool_message = "Não foi possível iniciar as tarefas do aplicativo".into();
        }
    }

    fn compare_folders(&mut self) {
        let left = self.compare_left.clone();
        let right = self.compare_right.clone();
        self.scanning_compare = true;
        self.compare_results.clear();
        self.tool_message = "Comparando nomes, tamanhos e conteúdo…".into();
        let tx = self.sender.clone();
        if let Some(runtime) = &self.runtime {
            runtime.spawn(async move {
                let left_worker = left.clone();
                let right_worker = right.clone();
                let result = tokio::task::spawn_blocking(move || {
                    file_engine::compare_directories(left_worker, right_worker)
                })
                .await
                .map_err(|error| error.to_string())
                .and_then(|result| result);
                let _ = tx.send(Event::Compared {
                    left,
                    right,
                    result,
                });
            });
        } else {
            self.scanning_compare = false;
            self.tool_message = "Não foi possível iniciar as tarefas do aplicativo".into();
        }
    }

    fn search_file_contents(&mut self) {
        let path = self.tool_root.clone();
        let query = self.content_query.clone();
        self.searching_content = true;
        self.content_results.clear();
        self.tool_message = "Procurando dentro de arquivos de texto…".into();
        let tx = self.sender.clone();
        if let Some(runtime) = &self.runtime {
            runtime.spawn(async move {
                let worker_path = path.clone();
                let result = tokio::task::spawn_blocking(move || {
                    file_engine::search_text_contents(worker_path, &query)
                })
                .await
                .map_err(|error| error.to_string())
                .and_then(|result| result);
                let _ = tx.send(Event::ContentSearch { path, result });
            });
        } else {
            self.searching_content = false;
            self.tool_message = "Não foi possível iniciar as tarefas do aplicativo".into();
        }
    }

    fn open_terminal(&mut self, shell: &str) {
        let path = self.active_directory().path.clone();
        #[cfg(windows)]
        let result = match shell {
            "cmd" => std::process::Command::new("cmd.exe")
                .current_dir(&path)
                .spawn()
                .map(|_| ()),
            "wsl" => {
                let raw = path.to_string_lossy().replace('\\', "/");
                let linux_path = if raw.len() >= 2 && raw.as_bytes()[1] == b':' {
                    format!(
                        "/mnt/{}/{}",
                        raw[..1].to_lowercase(),
                        raw[3..].trim_start_matches('/')
                    )
                } else {
                    raw
                };
                std::process::Command::new("wsl.exe")
                    .args(["--cd", &linux_path])
                    .spawn()
                    .map(|_| ())
            }
            _ => std::process::Command::new("powershell.exe")
                .current_dir(&path)
                .spawn()
                .map(|_| ()),
        };
        #[cfg(not(windows))]
        let result = std::process::Command::new("x-terminal-emulator")
            .current_dir(&path)
            .spawn()
            .map(|_| ());
        self.status = result
            .map(|_| format!("Terminal aberto em {}", path.display()))
            .unwrap_or_else(|error| format!("Não foi possível abrir o terminal: {error}"));
    }

    fn start_transfer(&mut self, sources: Vec<PathBuf>, destination: PathBuf, cut: bool) {
        let mut transfers = Vec::new();
        for source in sources {
            let Some(name) = source.file_name() else {
                continue;
            };
            let target = unique_destination(&destination.join(name));
            if target == source || target.starts_with(&source) {
                self.status = "Não é possível copiar uma pasta para dentro dela mesma".into();
                return;
            }
            transfers.push((source, target));
        }
        if transfers.is_empty() {
            self.status = "Nenhum item disponível para copiar".into();
            return;
        }
        self.dialog = Some(Dialog::Transfer(transfers, cut));
    }

    fn perform_transfer(&mut self, transfers: Vec<(PathBuf, PathBuf)>, cut: bool) {
        let count = transfers.len();
        let description = if cut {
            format!("{count} item(ns) movido(s)")
        } else {
            format!("Cópia de {count} item(ns) concluída")
        };
        self.run_operation(&description, move || {
            for (source, destination) in transfers {
                copy_path(&source, &destination).map_err(|e| format!("Falha ao copiar ‘{}’: {e}", source.display()))?;
                if cut { trash::delete(&source).map_err(|e| format!("A cópia foi criada, mas não foi possível mover o original para a Lixeira: {e}"))?; }
            }
            Ok(())
        });
    }

    fn run_operation(
        &mut self,
        description: &str,
        operation: impl FnOnce() -> Result<(), String> + Send + 'static,
    ) {
        if let Some(runtime) = &self.runtime {
            let tx = self.sender.clone();
            let description = description.to_owned();
            runtime.spawn(async move {
                let result = tokio::task::spawn_blocking(operation)
                    .await
                    .map_err(|error| error.to_string())
                    .and_then(|result| result);
                let _ = tx.send(Event::Operation {
                    description,
                    result,
                });
            });
        } else {
            self.status = "Não foi possível iniciar as tarefas do aplicativo".into();
        }
    }

    fn submit_rename(&mut self, path: &Path, pane: usize) {
        let name = self.rename_text.trim().to_owned();
        if name.is_empty()
            || Path::new(&name).file_name().and_then(|part| part.to_str()) != Some(name.as_str())
        {
            self.status = "Digite um nome de arquivo válido".into();
            return;
        }
        let source = path.to_path_buf();
        if pane == 0 {
            self.current_mut().selected.clear();
        } else {
            self.right.selected.clear();
        }
        self.dialog = None;
        self.run_operation("Renomeação concluída", move || {
            rename_path(&source, &name)
                .map(|_| ())
                .map_err(|error| format!("Não foi possível renomear: {error}"))
        });
    }

    fn clear_selection_paths(&mut self, paths: &[PathBuf]) {
        self.current_mut()
            .selected
            .retain(|item| !paths.contains(item));
        self.right.selected.retain(|item| !paths.contains(item));
    }

    fn run_action(&mut self, action: UiAction, ctx: &egui::Context) {
        match action {
            UiAction::Open(path, pane) => {
                if path.is_dir() {
                    self.navigate(pane, path);
                } else if let Err(error) = open::that(&path) {
                    self.status = format!("Não foi possível abrir o arquivo: {error}");
                }
            }
            UiAction::Favorite(path) => {
                if !path.is_dir() {
                    self.status =
                        "Os Favoritos guardam pastas. Escolha uma pasta para adicionar.".into();
                    return;
                }
                if !self.settings.favorites.contains(&path) {
                    self.settings.favorites.push(path);
                    self.persist();
                    self.status = "Atalho adicionado aos Favoritos".into();
                }
            }
            UiAction::CopyPath(path) => {
                ctx.copy_text(path.display().to_string());
                self.status = "Caminho copiado".into();
            }
            UiAction::Rename(path, pane) => {
                self.rename_text = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                self.dialog = Some(Dialog::Rename(path, pane));
            }
            UiAction::Delete(path) => {
                self.run_action(UiAction::DeleteMany(vec![path]), ctx);
            }
            UiAction::DeleteMany(paths) => {
                if self.settings.confirm_delete {
                    self.dialog = Some(Dialog::Delete(paths));
                } else {
                    self.clear_selection_paths(&paths);
                    self.run_operation("Itens enviados para a Lixeira", move || {
                        for path in paths {
                            trash::delete(&path).map_err(|e| e.to_string())?;
                        }
                        Ok(())
                    });
                }
            }
            UiAction::NewFolder(pane) => {
                self.folder_text.clear();
                self.dialog = Some(Dialog::NewFolder(pane));
            }
            UiAction::Copy(path, cut) => {
                self.run_action(UiAction::CopyMany(vec![path], cut), ctx);
            }
            UiAction::CopyMany(sources, cut) => {
                let count = sources.len();
                self.transfer = Some(Transfer { sources, cut });
                self.status = format!(
                    "{count} item(ns) {}. Navegue até o destino e escolha Colar.",
                    if cut {
                        "pronto(s) para mover"
                    } else {
                        "copiado(s)"
                    }
                );
            }
            UiAction::Paste => {
                let destination = if self.active_pane == 0 {
                    self.current().path.clone()
                } else {
                    self.right.path.clone()
                };
                if let Some(transfer) = self.transfer.clone() {
                    if transfer.cut
                        && transfer
                            .sources
                            .iter()
                            .all(|source| source.parent() == Some(destination.as_path()))
                    {
                        self.status = "Esses itens já estão nesta pasta".into();
                        return;
                    }
                    self.start_transfer(transfer.sources, destination, transfer.cut);
                } else {
                    self.status = "Primeiro escolha Copiar ou Recortar em um item".into();
                }
            }
            UiAction::RunPlugin(path, pane, index) => {
                let name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                match self
                    .plugins
                    .run(index, &name, &path.display().to_string(), path.is_dir())
                {
                    Ok(command) => self.handle_plugin_command(command, path, pane, ctx),
                    Err(error) => self.status = format!("Plugin: {error}"),
                }
            }
        }
    }

    fn sidebar(&mut self, ctx: &egui::Context) {
        let mut target = None;
        egui::SidePanel::left("navigation")
            .resizable(true)
            .default_width(220.0)
            .min_width(170.0)
            .show(ctx, |ui| {
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new(localization::tr("NAVEGAÇÃO", "PLACES"))
                        .size(12.0)
                        .strong()
                        .color(egui::Color32::GRAY),
                );
                ui.add_space(6.0);
                if let Some(dirs) = directories::UserDirs::new() {
                    for (icon, label, path) in [
                        (
                            IconKind::Home,
                            localization::tr("Área de Trabalho", "Desktop"),
                            dirs.desktop_dir(),
                        ),
                        (IconKind::Download, "Downloads", dirs.download_dir()),
                        (
                            IconKind::Document,
                            localization::tr("Documentos", "Documents"),
                            dirs.document_dir(),
                        ),
                        (
                            IconKind::Image,
                            localization::tr("Imagens", "Pictures"),
                            dirs.picture_dir(),
                        ),
                        (
                            IconKind::Audio,
                            localization::tr("Música", "Music"),
                            dirs.audio_dir(),
                        ),
                        (
                            IconKind::Video,
                            localization::tr("Vídeos", "Videos"),
                            dirs.video_dir(),
                        ),
                    ] {
                        let Some(path) = path else { continue };
                        let (rect, response) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), 40.0),
                            egui::Sense::click(),
                        );
                        paint_nav_row(
                            ui,
                            rect,
                            &response,
                            icon,
                            label,
                            accent_color(&self.settings),
                        );
                        if response.clicked() && path.exists() {
                            target = Some(path.to_path_buf());
                        }
                    }
                    let (rect, response) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), 40.0),
                        egui::Sense::click(),
                    );
                    paint_nav_row(
                        ui,
                        rect,
                        &response,
                        IconKind::Trash,
                        localization::tr("Lixeira", "Recycle Bin"),
                        accent_color(&self.settings),
                    );
                    if response.clicked() {
                        #[cfg(windows)]
                        {
                            let _ = std::process::Command::new("explorer.exe")
                                .arg("shell:RecycleBinFolder")
                                .spawn();
                        }
                        self.status = "Lixeira aberta no Explorador do Windows".into();
                    }
                }
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(localization::tr("FAVORITOS", "FAVORITES"))
                            .size(12.0)
                            .strong()
                            .color(egui::Color32::GRAY),
                    );
                    if ui
                        .small_button("+")
                        .on_hover_text("Adicionar pasta atual aos favoritos")
                        .clicked()
                        && !self.settings.favorites.contains(&self.current().path)
                    {
                        self.settings.favorites.push(self.current().path.clone());
                        self.persist();
                    }
                });
                for (i, path) in self.settings.favorites.iter().enumerate() {
                    let label = format!(
                        "★  {}",
                        path.file_name().unwrap_or_default().to_string_lossy()
                    );
                    let response = ui.add_sized(
                        [ui.available_width(), 34.0],
                        egui::Button::new(label).fill(egui::Color32::TRANSPARENT),
                    );
                    if response.clicked() {
                        target = Some(path.clone());
                    }
                    response.context_menu(|ui| {
                        if ui
                            .button(localization::tr(
                                "Remover dos favoritos",
                                "Remove from favorites",
                            ))
                            .clicked()
                        {
                            target = Some(PathBuf::from(format!("__remove_favorite_{i}")));
                            ui.close_menu();
                        }
                    });
                }
                #[cfg(windows)]
                {
                    ui.add_space(14.0);
                    ui.label(
                        egui::RichText::new(localization::tr("UNIDADES", "DRIVES"))
                            .size(12.0)
                            .strong()
                            .color(egui::Color32::GRAY),
                    );
                    for letter in b'A'..=b'Z' {
                        let drive = PathBuf::from(format!("{}:\\", letter as char));
                        if drive.exists()
                            && ui
                                .add_sized(
                                    [ui.available_width(), 34.0],
                                    egui::Button::new(format!(
                                        "{}  {} {}",
                                        "▰",
                                        localization::tr("Unidade", "Drive"),
                                        letter
                                    ))
                                    .fill(egui::Color32::TRANSPARENT),
                                )
                                .clicked()
                        {
                            target = Some(drive);
                        }
                    }
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    ui.label(
                        egui::RichText::new(localization::tr(
                            "Dica: clique duas vezes para abrir",
                            "Tip: double-click to open",
                        ))
                        .size(12.0)
                        .weak(),
                    );
                });
            });
        if let Some(path) = target {
            if let Some(value) = path
                .to_str()
                .and_then(|s| s.strip_prefix("__remove_favorite_"))
            {
                if let Ok(i) = value.parse::<usize>() {
                    if i < self.settings.favorites.len() {
                        self.settings.favorites.remove(i);
                        self.persist();
                    }
                }
            } else {
                self.navigate(0, path);
                self.screen = Screen::Files;
            }
        }
    }

    fn toolbar(&mut self, ctx: &egui::Context) {
        let mut navigate = None;
        let mut action = None;
        let mut terminal = None;
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.add_space(5.0);
            ui.horizontal_wrapped(|ui| {
                ui.heading(
                    egui::RichText::new("NORTHSTAR")
                        .size(22.0)
                        .color(accent_color(&self.settings)),
                );
                ui.separator();
                for (screen, label) in [
                    (Screen::Home, localization::tr("Início", "Home")),
                    (Screen::Files, localization::tr("Arquivos", "Files")),
                    (Screen::Tools, localization::tr("Ferramentas", "Tools")),
                    (
                        Screen::Customize,
                        localization::tr("Personalizar", "Customize"),
                    ),
                    (Screen::Plugins, "Plugins"),
                ] {
                    if ui.selectable_label(self.screen == screen, label).clicked() {
                        self.screen = screen;
                        if screen == Screen::Tools && self.tool_page == ToolPage::Uninstaller {
                            self.installed_programs = uninstaller::list_installed();
                        }
                    }
                }
                ui.separator();
                if ui.button(localization::tr("Nova aba", "New tab")).clicked() {
                    let path = self.current().path.clone();
                    self.tabs.push(Tab::at(path));
                    self.active_tab = self.tabs.len() - 1;
                    self.active_pane = 0;
                    self.path_input = self.current().path.display().to_string();
                    self.load(0);
                }
                if self.screen == Screen::Files {
                    if ui
                        .button(localization::tr("Nova pasta", "New folder"))
                        .clicked()
                    {
                        action = Some(UiAction::NewFolder(self.active_pane));
                    }
                    if ui.button(localization::tr("Colar", "Paste")).clicked() {
                        action = Some(UiAction::Paste);
                    }
                    if ui
                        .button(localization::tr("Espaço em pastas", "Folder sizes"))
                        .clicked()
                    {
                        self.tool_root = self.current().path.clone();
                        self.tool_page = ToolPage::FolderSizes;
                        self.screen = Screen::Tools;
                    }
                    ui.menu_button(localization::tr("Terminal", "Terminal"), |ui| {
                        if ui.button("PowerShell").clicked() {
                            terminal = Some("powershell");
                            ui.close_menu();
                        }
                        if ui
                            .button(localization::tr("Prompt de Comando", "Command Prompt"))
                            .clicked()
                        {
                            terminal = Some("cmd");
                            ui.close_menu();
                        }
                        if ui.button("WSL").clicked() {
                            terminal = Some("wsl");
                            ui.close_menu();
                        }
                    });
                    if ui
                        .button(localization::tr("Painel duplo", "Dual pane"))
                        .clicked()
                    {
                        self.dual_pane = !self.dual_pane;
                        if !self.dual_pane {
                            self.active_pane = 0;
                        } else {
                            self.load(1);
                        }
                        self.settings.dual_pane = self.dual_pane;
                        self.persist();
                    }
                }
                if ui.button(localization::tr("? Ajuda", "? Help")).clicked() {
                    self.show_help = true;
                }
            });
            if self.screen == Screen::Files {
                ui.horizontal(|ui| {
                    if ui.button(localization::tr("Voltar", "Back")).clicked() {
                        self.travel(false);
                    }
                    if ui.button(localization::tr("Avançar", "Forward")).clicked() {
                        self.travel(true);
                    }
                    if ui.button(localization::tr("Subir", "Up")).clicked() {
                        if let Some(parent) = self.current().path.parent() {
                            navigate = Some(parent.to_path_buf());
                        }
                    }
                    if ui
                        .button(localization::tr("Atualizar", "Refresh"))
                        .clicked()
                    {
                        self.load(0);
                    }
                    let response = ui.add_sized(
                        [ui.available_width() * 0.55, 32.0],
                        egui::TextEdit::singleline(&mut self.path_input)
                            .hint_text(localization::tr(
                                "Digite ou cole o caminho da pasta",
                                "Enter or paste a folder path",
                            ))
                            .id(egui::Id::new("path")),
                    );
                    if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        navigate = Some(PathBuf::from(self.path_input.trim()));
                    }
                    if ui.button("Ir").clicked() {
                        navigate = Some(PathBuf::from(self.path_input.trim()));
                    }
                    ui.add_sized(
                        [210.0, 32.0],
                        egui::TextEdit::singleline(&mut self.search)
                            .hint_text(localization::tr("Buscar nesta pasta", "Search this folder"))
                            .id(egui::Id::new("search")),
                    );
                });
                ui.horizontal(|ui| {
                    for i in 0..self.tabs.len() {
                        let title = self.tabs[i]
                            .path
                            .file_name()
                            .and_then(|x| x.to_str())
                            .unwrap_or("Início")
                            .to_owned();
                        if ui.selectable_label(self.active_tab == i, title).clicked() {
                            self.active_tab = i;
                            self.active_pane = 0;
                            self.path_input = self.current().path.display().to_string();
                        }
                        if self.tabs.len() > 1
                            && ui.small_button("×").on_hover_text("Fechar aba").clicked()
                        {
                            self.tabs.remove(i);
                            self.active_tab = self.active_tab.min(self.tabs.len() - 1);
                            self.active_pane = 0;
                            self.path_input = self.current().path.display().to_string();
                            break;
                        }
                    }
                });
                ui.collapsing("Filtros avançados", |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.add_sized(
                            [110.0, 28.0],
                            egui::TextEdit::singleline(&mut self.extension_filter)
                                .hint_text("Extensão (pdf)"),
                        );
                        ui.add_sized(
                            [120.0, 28.0],
                            egui::TextEdit::singleline(&mut self.min_size_filter)
                                .hint_text("Mínimo em bytes"),
                        );
                        ui.add_sized(
                            [120.0, 28.0],
                            egui::TextEdit::singleline(&mut self.max_size_filter)
                                .hint_text("Máximo em bytes"),
                        );
                        ui.add_sized(
                            [125.0, 28.0],
                            egui::TextEdit::singleline(&mut self.created_after_filter)
                                .hint_text("Criado após AAAA-MM-DD"),
                        );
                        ui.add_sized(
                            [125.0, 28.0],
                            egui::TextEdit::singleline(&mut self.created_before_filter)
                                .hint_text("Criado antes AAAA-MM-DD"),
                        );
                    });
                    ui.label("A extensão não precisa do ponto. Datas usam o formato AAAA-MM-DD.");
                    ui.horizontal(|ui| {
                        ui.add_sized(
                            [160.0, 26.0],
                            egui::TextEdit::singleline(&mut self.saved_filter_name)
                                .hint_text("Nome do filtro"),
                        );
                        if ui.button("Salvar filtro").clicked()
                            && !self.saved_filter_name.trim().is_empty()
                        {
                            self.settings.saved_filters.push(settings::SavedFilter {
                                name: self.saved_filter_name.trim().to_owned(),
                                extension: self.extension_filter.clone(),
                                min_size: self.min_size_filter.clone(),
                                max_size: self.max_size_filter.clone(),
                                created_after: self.created_after_filter.clone(),
                                created_before: self.created_before_filter.clone(),
                            });
                            self.persist();
                        }
                        let mut selected_filter = None;
                        egui::ComboBox::from_id_salt("saved_filters")
                            .selected_text("Filtros salvos")
                            .show_ui(ui, |ui| {
                                for (index, filter) in
                                    self.settings.saved_filters.iter().enumerate()
                                {
                                    if ui.selectable_label(false, &filter.name).clicked() {
                                        selected_filter = Some(index);
                                    }
                                }
                            });
                        if let Some(index) = selected_filter {
                            let filter = &self.settings.saved_filters[index];
                            self.extension_filter = filter.extension.clone();
                            self.min_size_filter = filter.min_size.clone();
                            self.max_size_filter = filter.max_size.clone();
                            self.created_after_filter = filter.created_after.clone();
                            self.created_before_filter = filter.created_before.clone();
                        }
                    });
                });
            }
        });
        if let Some(path) = navigate {
            self.navigate(0, path);
        }
        if let Some(action) = action {
            self.run_action(action, ctx);
        }
        if let Some(shell) = terminal {
            self.open_terminal(shell);
        }
    }

    fn draw_pane(&mut self, ui: &mut egui::Ui, pane: usize, ctx: &egui::Context) {
        let (path, entries, loading, error, selected) = if pane == 0 {
            let t = &self.tabs[self.active_tab];
            (
                t.path.clone(),
                &t.entries,
                t.loading,
                t.error.clone(),
                t.selected.clone(),
            )
        } else {
            let t = &self.right;
            (
                t.path.clone(),
                &t.entries,
                t.loading,
                t.error.clone(),
                t.selected.clone(),
            )
        };
        let needle = self.search.to_lowercase();
        let mut action = None;
        let mut selection = None;
        let ctrl_held = ctx.input(|input| input.modifiers.ctrl);
        let mut view_changed = false;
        let plugin_options: Vec<_> = self
            .plugins
            .plugins
            .iter()
            .enumerate()
            .filter(|(_, plugin)| plugin.enabled)
            .map(|(i, plugin)| (i, plugin.name.clone()))
            .collect();
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(if pane == 0 {
                    localization::tr("PASTA ATUAL", "CURRENT FOLDER")
                } else {
                    localization::tr("SEGUNDO PAINEL", "SECOND PANE")
                })
                .size(11.0)
                .strong()
                .color(egui::Color32::GRAY),
            );
            if ui
                .selectable_label(
                    self.active_pane == pane,
                    egui::RichText::new(path.display().to_string()).strong(),
                )
                .clicked()
            {
                self.active_pane = pane;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                egui::ComboBox::from_id_salt(("sort", pane))
                    .selected_text(match self.sort_by {
                        SortBy::Name => localization::tr("Nome", "Name"),
                        SortBy::Size => localization::tr("Tamanho", "Size"),
                        SortBy::Created => localization::tr("Criação", "Created"),
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.sort_by,
                            SortBy::Name,
                            localization::tr("Ordenar por nome", "Sort by name"),
                        );
                        ui.selectable_value(
                            &mut self.sort_by,
                            SortBy::Size,
                            localization::tr("Ordenar por tamanho", "Sort by size"),
                        );
                        ui.selectable_value(
                            &mut self.sort_by,
                            SortBy::Created,
                            localization::tr("Ordenar por criação", "Sort by creation date"),
                        );
                    });
                if ui
                    .selectable_label(
                        self.settings.view == ViewMode::List,
                        localization::tr("Lista", "List"),
                    )
                    .clicked()
                {
                    self.settings.view = ViewMode::List;
                    view_changed = true;
                }
                if ui
                    .selectable_label(
                        self.settings.view == ViewMode::Grid,
                        localization::tr("Grade", "Grid"),
                    )
                    .clicked()
                {
                    self.settings.view = ViewMode::Grid;
                    view_changed = true;
                }
            });
        });
        ui.separator();
        if loading {
            ui.add_space(30.0);
            ui.centered_and_justified(|ui| {
                ui.spinner();
                ui.label(localization::tr(
                    "Abrindo esta pasta…",
                    "Opening this folder…",
                ));
            });
        } else if let Some(error) = error {
            ui.colored_label(egui::Color32::LIGHT_RED, error);
            ui.label(localization::tr(
                "Confira o caminho ou suas permissões e tente novamente.",
                "Check the path or permissions and try again.",
            ));
        } else {
            let mut visible: Vec<&DirectoryEntry> = entries
                .iter()
                .filter(|e| self.settings.show_hidden || !e.is_hidden)
                .filter(|e| self.entry_matches(e, &needle))
                .collect();
            match self.sort_by {
                SortBy::Name => visible.sort_by(|a, b| {
                    b.is_dir
                        .cmp(&a.is_dir)
                        .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                }),
                SortBy::Size => visible.sort_by(|a, b| {
                    b.is_dir.cmp(&a.is_dir).then_with(|| {
                        let a_folder = (self.folder_sizes_root.as_deref() == a.path.parent())
                            .then(|| self.folder_sizes.get(&a.path).copied())
                            .flatten();
                        let b_folder = (self.folder_sizes_root.as_deref() == b.path.parent())
                            .then(|| self.folder_sizes.get(&b.path).copied())
                            .flatten();
                        let a_size = a.size.or(a_folder);
                        let b_size = b.size.or(b_folder);
                        b_size.cmp(&a_size)
                    })
                }),
                SortBy::Created => visible.sort_by(|a, b| {
                    b.is_dir
                        .cmp(&a.is_dir)
                        .then_with(|| b.created.cmp(&a.created))
                }),
            }
            ui.label(format!(
                "{} itens   ·   {}",
                visible.len(),
                if self.search.is_empty() {
                    "nesta pasta"
                } else {
                    "correspondências"
                }
            ));
            if selected.is_empty() {
                ui.label(
                    egui::RichText::new(localization::tr(
                        "Dica: Ctrl + clique seleciona vários itens",
                        "Tip: Ctrl-click to select multiple items",
                    ))
                    .weak()
                    .size(12.0),
                );
            }
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if visible.is_empty() {
                        ui.add_space(36.0);
                        ui.vertical_centered(|ui| {
                            ui.heading(if entries.is_empty() {
                                localization::tr("Esta pasta está vazia", "This folder is empty")
                            } else {
                                localization::tr("Nenhum item encontrado", "No items found")
                            });
                            ui.label(if entries.is_empty() {
                                localization::tr(
                                    "Crie uma pasta ou escolha outro lugar no menu à esquerda.",
                                    "Create a folder or choose another place from the left menu.",
                                )
                            } else {
                                localization::tr(
                                    "Tente buscar por outro nome.",
                                    "Try a different search term.",
                                )
                            });
                        });
                    }
                    if self.settings.view == ViewMode::Grid {
                        let columns = ((ui.available_width() / 160.0).floor() as usize).clamp(1, 4);
                        let total = visible.len();
                        egui::Grid::new(("file_grid", pane))
                            .num_columns(columns)
                            .spacing([12.0, 10.0])
                            .show(ui, |ui| {
                                for (index, item) in visible.into_iter().enumerate() {
                                    let (rect, response) = ui.allocate_exact_size(
                                        egui::vec2(140.0, 86.0),
                                        egui::Sense::click(),
                                    );
                                    ui.painter().rect_filled(
                                        rect,
                                        10.0,
                                        if selected.contains(&item.path) {
                                            accent_color(&self.settings).gamma_multiply(0.22)
                                        } else {
                                            ui.visuals().faint_bg_color
                                        },
                                    );
                                    paint_icon(
                                        ui.painter(),
                                        egui::Rect::from_min_size(
                                            rect.min + egui::vec2(52.0, 7.0),
                                            egui::vec2(36.0, 34.0),
                                        ),
                                        file_icon(item),
                                        icon_color(item),
                                    );
                                    ui.painter().text(
                                        egui::pos2(rect.center().x, rect.bottom() - 17.0),
                                        egui::Align2::CENTER_CENTER,
                                        truncate(&item.name, 20),
                                        egui::FontId::proportional(12.0),
                                        ui.visuals().text_color(),
                                    );
                                    if response.clicked() {
                                        selection = Some((item.path.clone(), ctrl_held));
                                    }
                                    if response.double_clicked() {
                                        action = Some(UiAction::Open(item.path.clone(), pane));
                                    }
                                    response.context_menu(|ui| {
                                        file_context(ui, item, pane, &plugin_options, &mut action)
                                    });
                                    if (index + 1) % columns == 0 || index + 1 == total {
                                        ui.end_row();
                                    }
                                }
                            });
                    } else {
                        ui.horizontal(|ui| {
                            if ui.button(localization::tr("Nome", "Name")).clicked() {
                                self.sort_by = SortBy::Name;
                            }
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .add_sized(
                                            [105.0, 26.0],
                                            egui::Button::new(localization::tr(
                                                "Criado", "Created",
                                            )),
                                        )
                                        .clicked()
                                    {
                                        self.sort_by = SortBy::Created;
                                    }
                                    if ui
                                        .add_sized(
                                            [80.0, 26.0],
                                            egui::Button::new(localization::tr("Tamanho", "Size")),
                                        )
                                        .clicked()
                                    {
                                        self.sort_by = SortBy::Size;
                                    }
                                },
                            );
                        });
                        for item in visible {
                            let selected_now = selected.contains(&item.path);
                            let icon = file_icon(item);
                            let label = if self.settings.show_file_extensions {
                                item.name.clone()
                            } else {
                                display_name_without_extension(item)
                            };
                            let size = item
                                .size
                                .filter(|_| !item.is_dir)
                                .or_else(|| {
                                    (self.folder_sizes_root.as_deref() == item.path.parent())
                                        .then(|| self.folder_sizes.get(&item.path).copied())
                                        .flatten()
                                })
                                .map(format_size)
                                .unwrap_or_default();
                            let height = if self.settings.compact_rows {
                                30.0
                            } else {
                                40.0
                            };
                            let row_width = ui.available_width();
                            let (rect, response) = ui.allocate_exact_size(
                                egui::vec2(row_width, height),
                                egui::Sense::click(),
                            );
                            let hovered = response.hovered();
                            if selected_now || hovered {
                                let fill = if selected_now {
                                    accent_color(&self.settings).gamma_multiply(0.25)
                                } else {
                                    ui.visuals().widgets.hovered.bg_fill
                                };
                                ui.painter().rect_filled(rect, 8.0, fill);
                            }
                            let center_y = rect.center().y;
                            let icon_rect = egui::Rect::from_center_size(
                                egui::pos2(rect.left() + 24.0, center_y),
                                egui::vec2(30.0, 30.0),
                            );
                            paint_icon(ui.painter(), icon_rect, icon, icon_color(item));
                            let narrow = row_width < 480.0;
                            let date_width = if narrow { 0.0 } else { 150.0 };
                            let size_width = if narrow { 0.0 } else { 85.0 };
                            let name_right = rect.right() - date_width - size_width - 22.0;
                            let name_rect = egui::Rect::from_min_max(
                                egui::pos2(icon_rect.right() + 12.0, rect.top()),
                                egui::pos2(name_right.max(icon_rect.right() + 30.0), rect.bottom()),
                            );
                            ui.painter().text(
                                egui::pos2(name_rect.left(), center_y),
                                egui::Align2::LEFT_CENTER,
                                label,
                                egui::FontId::proportional(14.0),
                                ui.visuals().text_color(),
                            );
                            if !narrow {
                                ui.painter().text(
                                    egui::pos2(
                                        rect.right() - date_width - size_width / 2.0,
                                        center_y,
                                    ),
                                    egui::Align2::CENTER_CENTER,
                                    size.clone(),
                                    egui::FontId::proportional(12.0),
                                    ui.visuals().weak_text_color(),
                                );
                                ui.painter().text(
                                    egui::pos2(rect.right() - date_width / 2.0 - 8.0, center_y),
                                    egui::Align2::CENTER_CENTER,
                                    format_modified(item.created),
                                    egui::FontId::proportional(12.0),
                                    ui.visuals().weak_text_color(),
                                );
                            }
                            response.clone().on_hover_text(format!(
                                "{}\nCriado: {}\nTamanho: {}",
                                item.name,
                                format_modified(item.created),
                                size
                            ));
                            if response.clicked() {
                                selection = Some((item.path.clone(), ctrl_held));
                            }
                            if response.double_clicked() {
                                action = Some(UiAction::Open(item.path.clone(), pane));
                            }
                            response.context_menu(|ui| {
                                file_context(ui, item, pane, &plugin_options, &mut action)
                            });
                            if let Some(message) = &item.error {
                                response.on_hover_text(message);
                            }
                        }
                    }
                });
        }
        if let Some((path, toggle)) = selection {
            self.active_pane = pane;
            let tab = if pane == 0 {
                self.current_mut()
            } else {
                &mut self.right
            };
            if toggle {
                if let Some(index) = tab.selected.iter().position(|item| item == &path) {
                    tab.selected.remove(index);
                } else {
                    tab.selected.push(path);
                }
            } else {
                tab.selected = vec![path];
            }
        }
        if let Some(action) = action {
            self.run_action(action, ctx);
        }
        if view_changed {
            self.persist();
        }
    }

    fn customize_screen(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.heading(localization::tr(
            "Personalize seu Northstar",
            "Customize Northstar",
        ));
        ui.label(localization::tr(
            "Deixe o aplicativo confortável para os seus olhos e para o seu jeito de trabalhar.",
            "Adjust the app to suit your eyes and workflow.",
        ));
        ui.label(format!(
            "{}: {}",
            localization::tr("Idioma detectado", "Detected language"),
            localization::detected_language_name()
        ));
        ui.add_space(14.0);
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.group(|ui| {
                ui.heading(localization::tr("Aparência", "Appearance"));
                ui.label(localization::tr(
                    "Escolha o tema que você prefere.",
                    "Choose your preferred theme.",
                ));
                ui.horizontal(|ui| {
                    for (theme, label) in [
                        (ThemeChoice::Light, localization::tr("Claro", "Light")),
                        (ThemeChoice::Dark, localization::tr("Escuro", "Dark")),
                        (ThemeChoice::System, localization::tr("Sistema", "System")),
                    ] {
                        if ui
                            .add_sized(
                                [120.0, 42.0],
                                egui::Button::new(label).selected(self.settings.theme == theme),
                            )
                            .clicked()
                        {
                            self.settings.theme = theme;
                            self.persist();
                            apply_theme(ctx, &self.settings);
                        }
                    }
                });
                ui.add_space(8.0);
                ui.label("Cor de destaque");
                ui.horizontal(|ui| {
                    for color in [
                        [105, 145, 255],
                        [66, 184, 131],
                        [220, 142, 67],
                        [196, 106, 166],
                        [67, 177, 189],
                        [220, 91, 91],
                    ] {
                        let c = egui::Color32::from_rgb(color[0], color[1], color[2]);
                        if ui
                            .add_sized([42.0, 36.0], egui::Button::new("●").fill(c))
                            .clicked()
                        {
                            self.settings.accent = color;
                            self.persist();
                            apply_theme(ctx, &self.settings);
                        }
                    }
                });
                ui.add_space(8.0);
                ui.label(format!(
                    "Tamanho do texto: {:.0}%",
                    self.settings.font_scale * 100.0
                ));
                if ui
                    .add(
                        egui::Slider::new(&mut self.settings.font_scale, 0.85..=1.5)
                            .show_value(false),
                    )
                    .changed()
                {
                    self.persist();
                }
            });
            ui.add_space(10.0);
            ui.group(|ui| {
                ui.heading("Como os arquivos aparecem");
                ui.label("Use o que ficar mais fácil de enxergar.");
                ui.horizontal(|ui| {
                    if ui
                        .add_sized(
                            [130.0, 44.0],
                            egui::Button::new("Lista")
                                .selected(self.settings.view == ViewMode::List),
                        )
                        .clicked()
                    {
                        self.settings.view = ViewMode::List;
                        self.persist();
                    }
                    if ui
                        .add_sized(
                            [130.0, 44.0],
                            egui::Button::new("Grade")
                                .selected(self.settings.view == ViewMode::Grid),
                        )
                        .clicked()
                    {
                        self.settings.view = ViewMode::Grid;
                        self.persist();
                    }
                });
                ui.checkbox(
                    &mut self.settings.compact_rows,
                    "Mostrar linhas mais compactas",
                );
                ui.checkbox(&mut self.settings.show_hidden, "Mostrar arquivos ocultos");
                ui.checkbox(
                    &mut self.settings.show_file_extensions,
                    "Mostrar extensão dos arquivos (.pdf, .jpg…)",
                );
                ui.checkbox(
                    &mut self.settings.confirm_delete,
                    "Pedir confirmação antes de excluir",
                );
                ui.checkbox(&mut self.dual_pane, "Abrir com painel duplo");
                self.settings.dual_pane = self.dual_pane;
                if ui.button("Salvar preferências").clicked() {
                    self.persist();
                    self.status = "Preferências salvas".into();
                }
            });
            ui.add_space(10.0);
            ui.group(|ui| {
                ui.heading("Atalhos fáceis");
                ui.label("Ctrl + L   Ir para a barra de endereço");
                ui.label("Ctrl + F   Buscar nesta pasta");
                ui.label("Ctrl + T   Abrir uma nova aba");
                ui.label(
                    "Ctrl + clique   Selecionar vários itens    ·    Ctrl + A   Selecionar tudo",
                );
                ui.label("Alt + setas esquerda/direita: voltar ou avançar");
                ui.label("F2   Renomear item selecionado    ·    Delete   Enviar para a Lixeira");
            });
        });
    }

    fn plugins_screen(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.heading("Plugins feitos pela comunidade");
        ui.label("Crie pequenas ações personalizadas. Os scripts não recebem acesso direto aos seus arquivos ou à internet.");
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Criar plugin de exemplo").clicked() {
                self.plugins.draft_name = "Meu plugin".into();
                self.plugins.draft_source = plugins::EXAMPLE_PLUGIN.into();
                self.plugins.selected = None;
            }
            if ui.button("Exemplo de personalização").clicked() {
                self.plugins.draft_name = "Ajuste de leitura para PDF".into();
                self.plugins.draft_source = plugins::CUSTOMIZATION_PLUGIN.into();
                self.plugins.selected = None;
            }
            if ui.button("Importar arquivo .rhai").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Plugin Rhai", &["rhai"])
                    .pick_file()
                {
                    if let Ok(source) = std::fs::read_to_string(&path) {
                        self.plugins.draft_name = path
                            .file_stem()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .replace('_', " ");
                        self.plugins.draft_source = source;
                        self.plugins.message =
                            "Script carregado. Revise e salve para instalar.".into();
                    }
                }
            }
            if ui.button("Atualizar lista").clicked() {
                self.plugins.reload();
            }
        });
        ui.columns(2, |cols| {
            cols[0].group(|ui| {
                ui.heading("Instalados");
                if self.plugins.plugins.is_empty() {
                    ui.label("Nenhum plugin ainda. Crie o primeiro!");
                }
                for i in 0..self.plugins.plugins.len() {
                    let name = self.plugins.plugins[i].name.clone();
                    if ui
                        .selectable_label(self.plugins.selected == Some(i), format!("◈  {name}"))
                        .clicked()
                    {
                        self.plugins.select(i);
                    }
                }
                if let Some(index) = self.plugins.selected {
                    let mut enabled = self.plugins.plugins.get(index).is_some_and(|p| p.enabled);
                    if ui.checkbox(&mut enabled, "Plugin ativo").changed() {
                        self.plugins.set_enabled(index, enabled);
                    }
                }
                if self.plugins.selected.is_some()
                    && ui.button("Remover plugin selecionado").clicked()
                {
                    self.plugins.delete_selected();
                }
            });
            cols[1].group(|ui| {
                ui.heading("Criar ou editar");
                ui.label("Nome do plugin");
                ui.text_edit_singleline(&mut self.plugins.draft_name);
                ui.label("O que o plugin faz?");
                ui.label("Edite o script abaixo. Para começar, basta mudar a mensagem de exemplo.");
                ui.add(
                    egui::TextEdit::multiline(&mut self.plugins.draft_source)
                        .code_editor()
                        .desired_rows(17)
                        .desired_width(f32::INFINITY),
                );
                ui.horizontal(|ui| {
                    if ui.button("Validar e salvar").clicked() {
                        self.plugins.save_draft();
                    }
                    if ui.button("Usar com item selecionado").clicked() {
                        if let Some(path) = self.active_directory().selected.first().cloned() {
                            let name = path
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_string();
                            match self.plugins.run_selected(
                                &name,
                                &path.display().to_string(),
                                path.is_dir(),
                            ) {
                                Ok(command) => {
                                    self.handle_plugin_command(
                                        command,
                                        path,
                                        self.active_pane,
                                        ctx,
                                    );
                                }
                                Err(error) => self.plugins.message = error,
                            }
                        } else {
                            self.plugins.message =
                                "Selecione um arquivo primeiro na aba Arquivos".into();
                        }
                    }
                });
                ui.label(
                    egui::RichText::new(&self.plugins.message).color(accent_color(&self.settings)),
                );
            });
        });
        ui.add_space(8.0);
        ui.label("Comandos seguros disponíveis: copy_path, favorite, open e notify:sua mensagem. Os plugins ficam na pasta de dados do usuário.");
    }

    fn handle_plugin_command(
        &mut self,
        command: String,
        path: PathBuf,
        pane: usize,
        ctx: &egui::Context,
    ) {
        if command == "copy_path" {
            ctx.copy_text(path.display().to_string());
            self.status = "Plugin: caminho copiado".into();
        } else if command == "favorite" {
            self.run_action(UiAction::Favorite(path), ctx);
        } else if command == "open" {
            self.run_action(UiAction::Open(path, pane), ctx);
        } else if command == "toggle_hidden" {
            self.settings.show_hidden = !self.settings.show_hidden;
            let shown = self.settings.show_hidden;
            self.persist();
            self.status = if shown {
                "Arquivos ocultos agora estão visíveis"
            } else {
                "Arquivos ocultos foram escondidos"
            }
            .into();
        } else if let Some(value) = command.strip_prefix("theme:") {
            self.settings.theme = match value {
                "light" => ThemeChoice::Light,
                "dark" => ThemeChoice::Dark,
                "system" => ThemeChoice::System,
                _ => {
                    self.status = "Plugin pediu um tema inválido".into();
                    return;
                }
            };
            self.persist();
            apply_theme(ctx, &self.settings);
            self.status = "Tema alterado pelo plugin".into();
        } else if let Some(value) = command.strip_prefix("view:") {
            self.settings.view = match value {
                "list" => ViewMode::List,
                "grid" => ViewMode::Grid,
                _ => {
                    self.status = "Plugin pediu uma visualização inválida".into();
                    return;
                }
            };
            self.persist();
            self.status = "Visualização alterada pelo plugin".into();
        } else if let Some(value) = command.strip_prefix("font:") {
            match value.parse::<u8>() {
                Ok(percent) if (85..=150).contains(&percent) => {
                    self.settings.font_scale = percent as f32 / 100.0;
                    self.persist();
                    apply_theme(ctx, &self.settings);
                    self.status = format!("Texto ajustado para {percent}%");
                }
                _ => self.status = "Tamanho de texto permitido: 85 a 150%".into(),
            }
        } else if let Some(value) = command.strip_prefix("accent:#") {
            if value.len() == 6 {
                if let Ok(rgb) = u32::from_str_radix(value, 16) {
                    self.settings.accent = [(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8];
                    self.persist();
                    apply_theme(ctx, &self.settings);
                    self.status = "Cor de destaque alterada pelo plugin".into();
                    return;
                }
            }
            self.status = "Cor inválida. Use accent:#RRGGBB".into();
        } else if let Some(text) = command.strip_prefix("notify:") {
            self.status = text.to_string();
        } else {
            self.plugins.message =
                "Comando não reconhecido. Use copy_path, favorite, open ou notify:mensagem.".into();
        }
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.dialog.clone() else {
            return;
        };
        match dialog {
            Dialog::Delete(paths) => {
                egui::Window::new("Enviar para a Lixeira?")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                    .show(ctx, |ui| {
                        ui.label(if paths.len() == 1 {
                            format!(
                                "O item ‘{}’ será enviado para a Lixeira do sistema.",
                                paths[0].file_name().unwrap_or_default().to_string_lossy()
                            )
                        } else {
                            format!(
                                "{} itens serão enviados para a Lixeira do sistema.",
                                paths.len()
                            )
                        });
                        ui.horizontal(|ui| {
                            if ui.button("Cancelar").clicked() {
                                self.dialog = None;
                            }
                            if ui.button("Enviar para a Lixeira").clicked() {
                                self.dialog = None;
                                self.clear_selection_paths(&paths);
                                let deleting = paths.clone();
                                self.run_operation("Itens enviados para a Lixeira", move || {
                                    for path in deleting {
                                        trash::delete(&path).map_err(|e| e.to_string())?;
                                    }
                                    Ok(())
                                });
                            }
                        });
                    });
            }
            Dialog::Rename(path, pane) => {
                egui::Window::new("Renomear item")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                    .show(ctx, |ui| {
                        ui.label("Digite o novo nome:");
                        let response = ui.text_edit_singleline(&mut self.rename_text);
                        if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            self.submit_rename(&path, pane);
                        }
                        ui.horizontal(|ui| {
                            if ui.button("Cancelar").clicked() {
                                self.dialog = None;
                            }
                            if ui.button("Renomear").clicked() {
                                self.submit_rename(&path, pane);
                            }
                        });
                    });
            }
            Dialog::NewFolder(pane) => {
                egui::Window::new("Criar uma pasta")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                    .show(ctx, |ui| {
                        ui.label("Qual será o nome da nova pasta?");
                        ui.text_edit_singleline(&mut self.folder_text);
                        ui.horizontal(|ui| {
                            if ui.button("Cancelar").clicked() {
                                self.dialog = None;
                            }
                            if ui.button("Criar pasta").clicked() {
                                let name = self.folder_text.trim().to_string();
                                if !name.is_empty()
                                    && Path::new(&name).file_name().and_then(|part| part.to_str())
                                        == Some(name.as_str())
                                {
                                    let folder = if pane == 0 {
                                        self.current().path.clone()
                                    } else {
                                        self.right.path.clone()
                                    };
                                    let path = folder.join(name);
                                    self.dialog = None;
                                    self.run_operation("Pasta criada", move || {
                                        std::fs::create_dir(path).map_err(|e| {
                                            format!("Não foi possível criar a pasta: {e}")
                                        })
                                    });
                                } else {
                                    self.status =
                                        "Digite um nome de pasta simples, sem barras".into();
                                }
                            }
                        });
                    });
            }
            Dialog::Transfer(transfers, cut) => {
                let count = transfers.len();
                egui::Window::new(if cut {
                    "Confirmar movimentação"
                } else {
                    "Confirmar cópia"
                })
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(if cut {
                        format!("Mover {count} item(ns) para esta pasta?")
                    } else {
                        format!("Copiar {count} item(ns) para esta pasta?")
                    });
                    if let Some((_, destination)) = transfers.first() {
                        ui.label(
                            destination
                                .parent()
                                .unwrap_or(destination)
                                .display()
                                .to_string(),
                        );
                    }
                    ui.horizontal(|ui| {
                        if ui.button("Cancelar").clicked() {
                            self.dialog = None;
                        }
                        if ui
                            .button(if cut { "Mover itens" } else { "Copiar itens" })
                            .clicked()
                        {
                            self.dialog = None;
                            let paths: Vec<PathBuf> =
                                transfers.iter().map(|(source, _)| source.clone()).collect();
                            if cut {
                                self.clear_selection_paths(&paths);
                            }
                            self.perform_transfer(transfers.clone(), cut);
                        }
                    });
                });
            }
            Dialog::Uninstall(program) => {
                egui::Window::new("Confirmar desinstalação")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                    .show(ctx, |ui| {
                        ui.heading(&program.name);
                        ui.label("O Windows abrirá o desinstalador registrado por este aplicativo.");
                        ui.label("Revise as opções do desinstalador. O Northstar não apagará arquivos residuais automaticamente.");
                        ui.horizontal(|ui| {
                            if ui.button("Cancelar").clicked() { self.dialog = None; }
                            if ui.button("Abrir desinstalador").clicked() {
                                self.dialog = None;
                                self.status = match uninstaller::launch_uninstaller(&program.uninstall_command) {
                                    Ok(()) => format!("Desinstalador iniciado: {}", program.name),
                                    Err(error) => error,
                                };
                            }
                        });
                    });
            }
        }
    }
}

impl eframe::App for ExplorerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        apply_theme(ctx, &self.settings);
        self.poll();
        ctx.request_repaint_after(Duration::from_millis(100));
        self.handle_shortcuts(ctx);
        self.toolbar(ctx);
        if self.screen == Screen::Files {
            self.sidebar(ctx);
            egui::CentralPanel::default().show(ctx, |ui| {
                if self.dual_pane {
                    ui.columns(2, |cols| {
                        self.draw_pane(&mut cols[0], 0, ctx);
                        self.draw_pane(&mut cols[1], 1, ctx);
                    });
                } else {
                    self.draw_pane(ui, 0, ctx);
                }
            });
            egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(format!("{} itens", self.active_directory().entries.len()));
                    ui.separator();
                    if let Some(selected) = self.active_directory().selected.first() {
                        ui.label(format!(
                            "{} selecionado(s) · {}",
                            self.active_directory().selected.len(),
                            selected.file_name().unwrap_or_default().to_string_lossy()
                        ));
                        ui.separator();
                    }
                    ui.label(&self.status);
                });
            });
        } else {
            egui::CentralPanel::default().show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    if self.screen == Screen::Home {
                        self.home_screen(ui);
                    } else if self.screen == Screen::Tools {
                        self.tools_screen(ui, ctx);
                    } else if self.screen == Screen::Customize {
                        self.customize_screen(ui, ctx);
                    } else {
                        self.plugins_screen(ui, ctx);
                    }
                });
            });
        }
        self.dialogs(ctx);
        if self.show_help {
            egui::Window::new("Como usar o Northstar").open(&mut self.show_help).collapsible(false).resizable(false).show(ctx, |ui| {
                ui.heading("Seja bem-vindo!");
                ui.label("Abra uma pasta pelos botões da esquerda. Clique duas vezes em um item para abrir.");
                ui.label("Use ‘＋ Nova aba’ para manter vários lugares abertos ao mesmo tempo.");
                ui.label("Clique com o botão direito em um item para ver as ações disponíveis.");
                ui.label("A busca filtra os nomes dentro da pasta atual.");
                ui.label("Em Personalizar, aumente o texto e escolha o tema mais confortável.");
                ui.label("Em Plugins, crie ações extras ou edite scripts de exemplo.");
            });
        }
    }
}

impl ExplorerApp {
    fn tools_screen(&mut self, ui: &mut egui::Ui, _ctx: &egui::Context) {
        ui.add_space(18.0);
        ui.heading(localization::tr(
            "Ferramentas para cuidar dos seus arquivos",
            "Tools for your files",
        ));
        ui.label(localization::tr(
            "Análises trabalham em segundo plano e não congelam a navegação.",
            "Scans run in the background so you can keep browsing.",
        ));
        ui.add_space(16.0);
        ui.horizontal(|ui| {
            for (page, label) in [
                (
                    ToolPage::FolderSizes,
                    localization::tr("Espaço em pastas", "Folder sizes"),
                ),
                (
                    ToolPage::Duplicates,
                    localization::tr("Arquivos duplicados", "Duplicates"),
                ),
                (
                    ToolPage::Compare,
                    localization::tr("Comparar pastas", "Compare folders"),
                ),
                (
                    ToolPage::ContentSearch,
                    localization::tr("Buscar no conteúdo", "Search contents"),
                ),
                (
                    ToolPage::BatchRename,
                    localization::tr("Renomear em lote", "Batch rename"),
                ),
                (
                    ToolPage::Uninstaller,
                    localization::tr("Desinstalador", "Uninstaller"),
                ),
            ] {
                if ui.selectable_label(self.tool_page == page, label).clicked() {
                    self.tool_page = page;
                    if page == ToolPage::Uninstaller && self.installed_programs.is_empty() {
                        self.installed_programs = uninstaller::list_installed();
                    }
                }
            }
        });
        ui.separator();

        match self.tool_page {
            ToolPage::FolderSizes => {
                ui.heading(localization::tr(
                    "Veja o que está ocupando espaço",
                    "See what's using your disk space",
                ));
                ui.label(localization::tr(
                    "Calcula o tamanho total de cada pasta filha, incluindo subpastas.",
                    "Calculates each child folder's total size, including subfolders.",
                ));
                ui.horizontal(|ui| {
                    ui.label(format!("Pasta: {}", self.tool_root.display()));
                    if ui
                        .button(localization::tr("Escolher pasta…", "Choose folder…"))
                        .clicked()
                    {
                        if let Some(path) = rfd::FileDialog::new()
                            .set_directory(&self.tool_root)
                            .pick_folder()
                        {
                            self.tool_root = path;
                            self.folder_sizes.clear();
                            self.folder_sizes_root = None;
                        }
                    }
                    if ui
                        .add_enabled(
                            !self.scanning_folders,
                            egui::Button::new(localization::tr(
                                "Calcular tamanhos",
                                "Calculate sizes",
                            )),
                        )
                        .clicked()
                    {
                        self.scan_folder_sizes();
                    }
                });
                if self.scanning_folders {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(localization::tr(
                            "Lendo pastas… você pode continuar usando o app.",
                            "Scanning folders… you can keep using the app.",
                        ));
                    });
                }
                ui.label(
                    egui::RichText::new(&self.tool_message).color(accent_color(&self.settings)),
                );
                let mut results: Vec<_> = self
                    .folder_sizes
                    .iter()
                    .filter(|(path, _)| path.parent() == Some(self.tool_root.as_path()))
                    .map(|(path, size)| (path.clone(), *size))
                    .collect();
                results.sort_by(|a, b| b.1.cmp(&a.1));
                let total_size: u64 = results.iter().map(|(_, size)| *size).sum();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (path, size) in results {
                        ui.horizontal(|ui| {
                            let (rect, _) = ui
                                .allocate_exact_size(egui::vec2(30.0, 30.0), egui::Sense::hover());
                            paint_icon(
                                ui.painter(),
                                rect,
                                IconKind::Folder,
                                egui::Color32::from_rgb(67, 126, 218),
                            );
                            ui.label(path.file_name().unwrap_or_default().to_string_lossy());
                            ui.add(
                                egui::ProgressBar::new(if total_size == 0 {
                                    0.0
                                } else {
                                    size as f32 / total_size as f32
                                })
                                .desired_width(160.0),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(format_size(size));
                                },
                            );
                        });
                        ui.separator();
                    }
                });
            }
            ToolPage::Duplicates => {
                ui.heading(localization::tr(
                    "Encontre cópias idênticas",
                    "Find identical copies",
                ));
                ui.label(localization::tr("Arquivos são comparados pelo conteúdo. Nenhum item será removido automaticamente.", "Files are compared by content. Nothing is removed automatically."));
                ui.horizontal(|ui| {
                    ui.label(format!("Pasta analisada: {}", self.tool_root.display()));
                    if ui
                        .button(localization::tr("Escolher pasta…", "Choose folder…"))
                        .clicked()
                    {
                        if let Some(path) = rfd::FileDialog::new()
                            .set_directory(&self.tool_root)
                            .pick_folder()
                        {
                            self.tool_root = path;
                            self.duplicate_groups.clear();
                        }
                    }
                    if ui
                        .add_enabled(
                            !self.scanning_duplicates,
                            egui::Button::new(localization::tr(
                                "Buscar duplicados",
                                "Find duplicates",
                            )),
                        )
                        .clicked()
                    {
                        self.scan_duplicates();
                    }
                });
                if self.scanning_duplicates {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(localization::tr("Comparando arquivos…", "Comparing files…"));
                    });
                }
                ui.label(
                    egui::RichText::new(&self.tool_message).color(accent_color(&self.settings)),
                );
                let reclaimable: u64 = self
                    .duplicate_groups
                    .iter()
                    .map(|group| {
                        group
                            .size
                            .saturating_mul(group.files.len().saturating_sub(1) as u64)
                    })
                    .sum();
                if !self.duplicate_groups.is_empty() {
                    ui.label(format!(
                        "Espaço recuperável se mantiver uma cópia de cada grupo: {}",
                        format_size(reclaimable)
                    ));
                }
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for group in &self.duplicate_groups {
                        ui.group(|ui| {
                            ui.horizontal(|ui| {
                                ui.strong(format!("{} cópias", group.files.len()));
                                ui.label(format_size(group.size));
                            });
                            for path in &group.files {
                                ui.label(path.display().to_string());
                            }
                        });
                    }
                });
            }
            ToolPage::Compare => {
                ui.heading(localization::tr(
                    "Compare duas pastas",
                    "Compare two folders",
                ));
                ui.label(localization::tr("Mostra itens exclusivos e arquivos com conteúdo diferente. Pastas internas não são percorridas.", "Shows unique items and files with different contents. Subfolders are not scanned."));
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .button(localization::tr("Escolher pasta A…", "Choose folder A…"))
                        .clicked()
                    {
                        if let Some(path) = rfd::FileDialog::new()
                            .set_directory(&self.compare_left)
                            .pick_folder()
                        {
                            self.compare_left = path;
                            self.compare_results.clear();
                        }
                    }
                    ui.label(self.compare_left.display().to_string());
                });
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .button(localization::tr("Escolher pasta B…", "Choose folder B…"))
                        .clicked()
                    {
                        if let Some(path) = rfd::FileDialog::new()
                            .set_directory(&self.compare_right)
                            .pick_folder()
                        {
                            self.compare_right = path;
                            self.compare_results.clear();
                        }
                    }
                    ui.label(self.compare_right.display().to_string());
                    if ui
                        .add_enabled(
                            !self.scanning_compare && self.compare_left != self.compare_right,
                            egui::Button::new(localization::tr("Comparar", "Compare")),
                        )
                        .clicked()
                    {
                        self.compare_folders();
                    }
                });
                if self.scanning_compare {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(localization::tr("Analisando pastas…", "Comparing folders…"));
                    });
                }
                ui.label(
                    egui::RichText::new(&self.tool_message).color(accent_color(&self.settings)),
                );
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for item in &self.compare_results {
                        ui.horizontal(|ui| {
                            ui.label(match item.difference.as_str() {
                                "somente_esquerda" => "Só em A",
                                "somente_direita" => "Só em B",
                                _ => "Conteúdo diferente",
                            });
                            ui.label(&item.name);
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let a = item
                                        .left_size
                                        .map(format_size)
                                        .unwrap_or_else(|| "—".into());
                                    let b = item
                                        .right_size
                                        .map(format_size)
                                        .unwrap_or_else(|| "—".into());
                                    ui.label(format!("A: {a}   ·   B: {b}"));
                                },
                            );
                        });
                        ui.separator();
                    }
                });
            }
            ToolPage::ContentSearch => {
                ui.heading(localization::tr(
                    "Encontre texto dentro dos arquivos",
                    "Find text inside files",
                ));
                ui.label(localization::tr("Busca recursiva em arquivos de texto UTF-8 de até 8 MiB; binários e arquivos maiores são ignorados.", "Searches recursively in UTF-8 text files up to 8 MiB; binary and larger files are skipped."));
                ui.horizontal(|ui| {
                    ui.label(format!("Pasta: {}", self.tool_root.display()));
                    if ui.button("Escolher pasta…").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .set_directory(&self.tool_root)
                            .pick_folder()
                        {
                            self.tool_root = path;
                            self.content_results.clear();
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.add_sized(
                        [360.0, 32.0],
                        egui::TextEdit::singleline(&mut self.content_query)
                            .hint_text("Palavra ou frase para localizar"),
                    );
                    if ui
                        .add_enabled(
                            !self.searching_content && !self.content_query.trim().is_empty(),
                            egui::Button::new("Buscar no conteúdo"),
                        )
                        .clicked()
                    {
                        self.search_file_contents();
                    }
                });
                if self.searching_content {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Lendo arquivos em segundo plano…");
                    });
                }
                ui.label(
                    egui::RichText::new(&self.tool_message).color(accent_color(&self.settings)),
                );
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for path in &self.content_results {
                        ui.label(path.display().to_string());
                    }
                });
            }
            ToolPage::BatchRename => {
                ui.heading("Renomeie vários itens com uma regra");
                ui.label(format!(
                    "{} itens selecionados na aba Arquivos",
                    self.active_directory().selected.len()
                ));
                ui.horizontal(|ui| {
                    ui.add_sized(
                        [190.0, 30.0],
                        egui::TextEdit::singleline(&mut self.rename_prefix)
                            .hint_text("Adicionar antes do nome"),
                    );
                    ui.add_sized(
                        [190.0, 30.0],
                        egui::TextEdit::singleline(&mut self.rename_suffix)
                            .hint_text("Adicionar depois do nome"),
                    );
                    if ui.button("Pré-visualizar").clicked() {
                        self.batch_preview = self
                            .active_directory()
                            .selected
                            .iter()
                            .filter_map(|path| {
                                let stem = path.file_stem()?.to_string_lossy();
                                let extension =
                                    path.extension().map(|e| e.to_string_lossy().into_owned());
                                let name = format!(
                                    "{}{}{}{}",
                                    self.rename_prefix,
                                    stem,
                                    self.rename_suffix,
                                    extension.map(|e| format!(".{e}")).unwrap_or_default()
                                );
                                Some((path.clone(), name))
                            })
                            .collect();
                    }
                    if ui
                        .add_enabled(
                            !self.batch_preview.is_empty(),
                            egui::Button::new("Renomear itens"),
                        )
                        .clicked()
                    {
                        let preview = self.batch_preview.clone();
                        let targets: Vec<PathBuf> = preview
                            .iter()
                            .filter_map(|(source, name)| {
                                source.parent().map(|parent| parent.join(name))
                            })
                            .collect();
                        let unique: std::collections::HashSet<_> = targets.iter().collect();
                        if unique.len() != targets.len()
                            || targets.iter().any(|target| target.exists())
                        {
                            self.tool_message =
                                "A prévia contém nomes que já existem. Ajuste o prefixo ou sufixo."
                                    .into();
                        } else {
                            self.batch_preview.clear();
                            self.run_operation("Renomeação em lote concluída", move || {
                                for (source, name) in preview {
                                    rename_path(&source, &name).map_err(|error| {
                                        format!("Falha ao renomear {}: {error}", source.display())
                                    })?;
                                }
                                Ok(())
                            });
                        }
                    }
                });
                ui.label("A extensão original é preservada. Confira a prévia antes de aplicar; conflitos de nomes impedem a execução.");
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (source, name) in &self.batch_preview {
                        ui.horizontal(|ui| {
                            ui.label(source.file_name().unwrap_or_default().to_string_lossy());
                            ui.label("vira");
                            ui.strong(name);
                        });
                    }
                });
                if !self.tool_message.is_empty() {
                    ui.label(
                        egui::RichText::new(&self.tool_message).color(accent_color(&self.settings)),
                    );
                }
            }
            ToolPage::Uninstaller => {
                ui.heading(localization::tr(
                    "Desinstale aplicativos instalados",
                    "Uninstall installed apps",
                ));
                ui.label(localization::tr("O Northstar executa o desinstalador que o próprio aplicativo registrou no Windows.", "Northstar runs the uninstaller registered by the app in Windows."));
                ui.label(localization::tr("A remoção de sobras não é automática: pastas e arquivos podem conter dados pessoais.", "Leftovers are not removed automatically; folders may contain personal data."));
                ui.horizontal(|ui| {
                    ui.add_sized(
                        [330.0, 32.0],
                        egui::TextEdit::singleline(&mut self.uninstall_filter).hint_text(
                            localization::tr(
                                "Filtrar por nome ou fabricante",
                                "Filter by name or publisher",
                            ),
                        ),
                    );
                    if ui
                        .button(localization::tr("Atualizar lista", "Refresh list"))
                        .clicked()
                    {
                        self.installed_programs = uninstaller::list_installed();
                    }
                    ui.label(format!("{} aplicativos", self.installed_programs.len()));
                });
                let needle = self.uninstall_filter.to_lowercase();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for program in self.installed_programs.iter().filter(|program| {
                        needle.is_empty()
                            || program.name.to_lowercase().contains(&needle)
                            || program.publisher.to_lowercase().contains(&needle)
                    }) {
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.strong(&program.name);
                                ui.label(format!(
                                    "{}{}",
                                    program.publisher,
                                    if program.version.is_empty() {
                                        String::new()
                                    } else {
                                        format!(" · {}", program.version)
                                    }
                                ));
                            });
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .button(localization::tr("Desinstalar…", "Uninstall…"))
                                        .clicked()
                                    {
                                        self.dialog = Some(Dialog::Uninstall(program.clone()));
                                    }
                                },
                            );
                        });
                        ui.separator();
                    }
                });
                #[cfg(not(windows))]
                ui.label(localization::tr(
                    "O catálogo de programas instalados requer Windows.",
                    "The installed apps catalog requires Windows.",
                ));
            }
        }
    }

    fn home_screen(&mut self, ui: &mut egui::Ui) {
        let accent = accent_color(&self.settings);
        ui.add_space(34.0);
        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new(localization::tr("SEU ESPAÇO DE TRABALHO", "YOUR WORKSPACE"))
                    .size(11.0)
                    .strong()
                    .color(accent),
            );
            ui.add_space(8.0);
            ui.heading(
                egui::RichText::new(localization::tr("Tudo à mão.", "Everything in one place."))
                    .size(34.0)
                    .strong(),
            );
            ui.label(
                egui::RichText::new(localization::tr(
                    "Acesse suas pastas e continue de onde parou.",
                    "Open your folders and pick up where you left off.",
                ))
                .size(15.0)
                .weak(),
            );
        });
        ui.add_space(32.0);
        ui.vertical_centered(|ui| {
            ui.colored_label(
                egui::Color32::from_rgb(235, 185, 92),
                localization::tr(
                    "VERSÃO EXPERIMENTAL · Pode conter bugs. Envie sugestões e relatos de problemas.",
                    "EXPERIMENTAL BUILD · May contain bugs. Send feedback and bug reports.",
                ),
            );
        });
        ui.add_space(12.0);
        let places: Vec<(IconKind, &str, Option<PathBuf>, egui::Color32)> =
            directories::UserDirs::new()
                .map(|dirs| {
                    vec![
                        (
                            IconKind::Home,
                            localization::tr("Área de Trabalho", "Desktop"),
                            dirs.desktop_dir().map(Path::to_path_buf),
                            egui::Color32::from_rgb(75, 132, 245),
                        ),
                        (
                            IconKind::Download,
                            "Downloads",
                            dirs.download_dir().map(Path::to_path_buf),
                            egui::Color32::from_rgb(38, 173, 146),
                        ),
                        (
                            IconKind::Document,
                            localization::tr("Documentos", "Documents"),
                            dirs.document_dir().map(Path::to_path_buf),
                            egui::Color32::from_rgb(157, 111, 235),
                        ),
                        (
                            IconKind::Image,
                            localization::tr("Imagens", "Pictures"),
                            dirs.picture_dir().map(Path::to_path_buf),
                            egui::Color32::from_rgb(237, 139, 65),
                        ),
                        (
                            IconKind::Audio,
                            localization::tr("Música", "Music"),
                            dirs.audio_dir().map(Path::to_path_buf),
                            egui::Color32::from_rgb(220, 87, 137),
                        ),
                        (
                            IconKind::Video,
                            localization::tr("Vídeos", "Videos"),
                            dirs.video_dir().map(Path::to_path_buf),
                            egui::Color32::from_rgb(48, 164, 190),
                        ),
                        (
                            IconKind::Trash,
                            localization::tr("Lixeira", "Recycle Bin"),
                            None,
                            egui::Color32::from_rgb(215, 101, 92),
                        ),
                    ]
                })
                .unwrap_or_default();
        let mut selected_path = None;
        ui.horizontal(|ui| {
            let grid_width = 3.0 * 210.0 + 2.0 * 16.0;
            ui.add_space(((ui.available_width() - grid_width) / 2.0).max(0.0));
            egui::Grid::new("home_places")
                .num_columns(3)
                .spacing([16.0, 16.0])
                .show(ui, |ui| {
                    for (index, (icon, label, path, color)) in places.iter().enumerate() {
                        let response = ui.allocate_ui_with_layout(
                            egui::vec2(210.0, 112.0),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                let (rect, response) = ui.allocate_exact_size(
                                    egui::vec2(210.0, 112.0),
                                    egui::Sense::click(),
                                );
                                let hovered = response.hovered();
                                let t = ui.ctx().animate_bool(response.id, hovered);
                                let fill = if hovered {
                                    ui.visuals().widgets.hovered.bg_fill
                                } else {
                                    ui.visuals().faint_bg_color
                                };
                                ui.painter().rect_filled(rect, 14.0, fill);
                                let badge = egui::Rect::from_min_size(
                                    rect.min + egui::vec2(14.0, 14.0 - 2.0 * t),
                                    egui::vec2(34.0, 34.0),
                                );
                                paint_icon(ui.painter(), badge, *icon, *color);
                                ui.painter().text(
                                    rect.min + egui::vec2(14.0, 70.0),
                                    egui::Align2::LEFT_CENTER,
                                    *label,
                                    egui::FontId::proportional(14.0),
                                    ui.visuals().text_color(),
                                );
                                ui.painter().text(
                                    rect.right_bottom() - egui::vec2(14.0, 18.0),
                                    egui::Align2::RIGHT_CENTER,
                                    localization::tr("Abrir pasta", "Open folder"),
                                    egui::FontId::proportional(12.0),
                                    accent,
                                );
                                if hovered {
                                    ui.ctx().request_repaint();
                                }
                                response
                            },
                        );
                        if response.inner.clicked() {
                            if let Some(path) = path {
                                selected_path = Some(path.clone());
                            } else {
                                #[cfg(windows)]
                                {
                                    let _ = std::process::Command::new("explorer.exe")
                                        .arg("shell:RecycleBinFolder")
                                        .spawn();
                                }
                                self.status = "Lixeira aberta no Explorador do Windows".into();
                            }
                        }
                        if index % 3 == 2 {
                            ui.end_row();
                        }
                    }
                })
        });
        ui.add_space(28.0);
        ui.heading(localization::tr("Pastas recentes", "Recent folders"));
        let mut recent_target = None;
        for path in self.settings.recent_paths.iter().take(6) {
            let label = path.file_name().unwrap_or_default().to_string_lossy();
            let response = ui.add_sized(
                [ui.available_width().min(650.0), 34.0],
                egui::Button::new(format!(
                    "{}     {}",
                    if path.is_dir() {
                        "Pasta"
                    } else {
                        "Indisponível"
                    },
                    label
                ))
                .fill(egui::Color32::TRANSPARENT),
            );
            if response.clicked() && path.is_dir() {
                recent_target = Some(path.clone());
            }
            response.on_hover_text(path.display().to_string());
        }
        if self.settings.recent_paths.is_empty() {
            ui.label(localization::tr(
                "As pastas que você abrir aparecerão aqui.",
                "Folders you open will appear here.",
            ));
        }
        if let Some(path) = recent_target {
            selected_path = Some(path);
        }
        if let Some(path) = selected_path {
            self.navigate(0, path);
            self.screen = Screen::Files;
        }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if self.screen != Screen::Files {
            return;
        }
        let mut action = None;
        let text_input_focused = ctx.wants_keyboard_input();
        ctx.input(|i| {
            if !text_input_focused && i.modifiers.ctrl && i.key_pressed(egui::Key::T) {
                let path = self.current().path.clone();
                self.tabs.push(Tab::at(path));
                self.active_tab = self.tabs.len() - 1;
                self.active_pane = 0;
                self.load(0);
            }
            if !text_input_focused && i.modifiers.ctrl && i.key_pressed(egui::Key::A) {
                let selected: Vec<PathBuf> = self
                    .active_directory()
                    .entries
                    .iter()
                    .filter(|entry| self.settings.show_hidden || !entry.is_hidden)
                    .filter(|entry| self.entry_matches(entry, &self.search.to_lowercase()))
                    .map(|entry| entry.path.clone())
                    .collect();
                self.active_directory_mut().selected = selected;
                self.status = format!(
                    "{} itens selecionados",
                    self.active_directory().selected.len()
                );
            }
            if i.modifiers.ctrl && i.key_pressed(egui::Key::F) {
                self.screen = Screen::Files;
                ctx.memory_mut(|m| m.request_focus(egui::Id::new("search")));
            }
            if i.modifiers.ctrl && i.key_pressed(egui::Key::L) {
                ctx.memory_mut(|m| m.request_focus(egui::Id::new("path")));
            }
            if !text_input_focused && i.modifiers.alt && i.key_pressed(egui::Key::ArrowLeft) {
                self.travel(false);
            }
            if !text_input_focused && i.modifiers.alt && i.key_pressed(egui::Key::ArrowRight) {
                self.travel(true);
            }
            if !text_input_focused && i.key_pressed(egui::Key::F2) {
                if let Some(path) = self.active_directory().selected.first().cloned() {
                    action = Some(UiAction::Rename(path, self.active_pane));
                }
            }
            if !text_input_focused && i.key_pressed(egui::Key::Delete) {
                if !self.active_directory().selected.is_empty() {
                    action = Some(UiAction::DeleteMany(
                        self.active_directory().selected.clone(),
                    ));
                }
            }
            if !text_input_focused && i.modifiers.ctrl && i.key_pressed(egui::Key::C) {
                if !self.active_directory().selected.is_empty() {
                    action = Some(UiAction::CopyMany(
                        self.active_directory().selected.clone(),
                        false,
                    ));
                }
            }
            if !text_input_focused && i.modifiers.ctrl && i.key_pressed(egui::Key::X) {
                if !self.active_directory().selected.is_empty() {
                    action = Some(UiAction::CopyMany(
                        self.active_directory().selected.clone(),
                        true,
                    ));
                }
            }
            if !text_input_focused && i.modifiers.ctrl && i.key_pressed(egui::Key::V) {
                action = Some(UiAction::Paste);
            }
        });
        if let Some(action) = action {
            self.run_action(action, ctx);
        }
    }
}

fn file_context(
    ui: &mut egui::Ui,
    item: &DirectoryEntry,
    pane: usize,
    plugins: &[(usize, String)],
    action: &mut Option<UiAction>,
) {
    if ui.button("Abrir").clicked() {
        *action = Some(UiAction::Open(item.path.clone(), pane));
        ui.close_menu();
    }
    if ui.button("Copiar").clicked() {
        *action = Some(UiAction::Copy(item.path.clone(), false));
        ui.close_menu();
    }
    if ui.button("Recortar").clicked() {
        *action = Some(UiAction::Copy(item.path.clone(), true));
        ui.close_menu();
    }
    if ui.button("Adicionar aos favoritos").clicked() {
        *action = Some(UiAction::Favorite(item.path.clone()));
        ui.close_menu();
    }
    if ui.button("Copiar caminho").clicked() {
        *action = Some(UiAction::CopyPath(item.path.clone()));
        ui.close_menu();
    }
    if ui.button("Renomear…").clicked() {
        *action = Some(UiAction::Rename(item.path.clone(), pane));
        ui.close_menu();
    }
    if ui.button("Excluir…").clicked() {
        *action = Some(UiAction::Delete(item.path.clone()));
        ui.close_menu();
    }
    if !plugins.is_empty() {
        ui.menu_button("Executar plugin", |ui| {
            for (index, name) in plugins {
                if ui.button(name).clicked() {
                    *action = Some(UiAction::RunPlugin(item.path.clone(), pane, *index));
                    ui.close_menu();
                }
            }
        });
    }
}

fn unique_destination(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }
    let parent = path.parent().unwrap_or(Path::new("."));
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Cópia");
    let ext = path.extension().and_then(|s| s.to_str());
    for n in 2..10000 {
        let name = match ext {
            Some(ext) => format!("{stem} ({n}).{ext}"),
            None => format!("{stem} ({n})"),
        };
        let candidate = parent.join(name);
        if !candidate.exists() {
            return candidate;
        }
    }
    parent.join(format!("{stem} (cópia)"))
}

fn display_name_without_extension(item: &DirectoryEntry) -> String {
    if item.is_dir {
        return item.name.clone();
    }
    item.path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(&item.name)
        .to_owned()
}

fn truncate(name: &str, max: usize) -> String {
    if name.chars().count() <= max {
        name.to_owned()
    } else {
        format!(
            "{}…",
            name.chars().take(max.saturating_sub(1)).collect::<String>()
        )
    }
}

fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut n = bytes as f64;
    let mut i = 0;
    while n >= 1024.0 && i < UNITS.len() - 1 {
        n /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{bytes} B")
    } else {
        format!("{n:.1} {}", UNITS[i])
    }
}

fn file_icon(item: &DirectoryEntry) -> IconKind {
    if item.is_dir {
        return IconKind::Folder;
    }
    match item
        .path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" => IconKind::Image,
        "zip" | "7z" | "rar" | "tar" | "gz" => IconKind::Archive,
        "exe" | "msi" | "app" => IconKind::App,
        "mp3" | "wav" | "flac" | "aac" => IconKind::Audio,
        "mp4" | "mkv" | "mov" | "avi" => IconKind::Video,
        "rs" | "ts" | "tsx" | "js" | "py" | "json" | "toml" => IconKind::Code,
        _ => IconKind::File,
    }
}

fn icon_color(item: &DirectoryEntry) -> egui::Color32 {
    match file_icon(item) {
        IconKind::Folder => egui::Color32::from_rgb(67, 126, 218),
        IconKind::Image => egui::Color32::from_rgb(191, 103, 194),
        IconKind::Archive => egui::Color32::from_rgb(194, 143, 57),
        IconKind::App => egui::Color32::from_rgb(71, 157, 126),
        IconKind::Audio | IconKind::Video => egui::Color32::from_rgb(67, 142, 190),
        IconKind::Code => egui::Color32::from_rgb(87, 113, 151),
        _ => egui::Color32::from_rgb(105, 119, 142),
    }
}

fn paint_icon(painter: &egui::Painter, rect: egui::Rect, kind: IconKind, color: egui::Color32) {
    use egui::{Color32, Pos2, Rect, Stroke, StrokeKind};
    painter.rect_filled(rect, 8.0, color);
    let white = Color32::WHITE;
    let stroke = Stroke::new((rect.width() / 14.0).clamp(1.2, 2.0), white);
    let p = |x: f32, y: f32| {
        Pos2::new(
            rect.left() + rect.width() * x,
            rect.top() + rect.height() * y,
        )
    };
    let r = |x: f32, y: f32, w: f32, h: f32| Rect::from_min_max(p(x, y), p(x + w, y + h));
    match kind {
        IconKind::Folder => {
            painter.rect_filled(r(0.16, 0.32, 0.28, 0.13), 2.0, white);
            painter.rect_filled(r(0.14, 0.40, 0.72, 0.38), 3.0, white);
            painter.rect_filled(r(0.19, 0.45, 0.62, 0.27), 2.0, color);
        }
        IconKind::Home => {
            painter.line_segment([p(0.18, 0.48), p(0.50, 0.20)], stroke);
            painter.line_segment([p(0.50, 0.20), p(0.82, 0.48)], stroke);
            painter.rect_stroke(r(0.27, 0.46, 0.46, 0.34), 2.0, stroke, StrokeKind::Middle);
            painter.rect_filled(r(0.45, 0.58, 0.14, 0.22), 1.0, white);
        }
        IconKind::Download => {
            painter.line_segment([p(0.50, 0.18), p(0.50, 0.58)], stroke);
            painter.line_segment([p(0.33, 0.43), p(0.50, 0.60)], stroke);
            painter.line_segment([p(0.67, 0.43), p(0.50, 0.60)], stroke);
            painter.line_segment([p(0.24, 0.68), p(0.76, 0.68)], stroke);
            painter.line_segment([p(0.24, 0.68), p(0.24, 0.78)], stroke);
            painter.line_segment([p(0.76, 0.68), p(0.76, 0.78)], stroke);
        }
        IconKind::Document
        | IconKind::File
        | IconKind::Code
        | IconKind::App
        | IconKind::Archive => {
            painter.rect_filled(r(0.28, 0.16, 0.46, 0.68), 3.0, white);
            painter.rect_filled(r(0.35, 0.30, 0.28, 0.035), 1.0, color);
            painter.rect_filled(r(0.35, 0.42, 0.28, 0.035), 1.0, color);
            painter.rect_filled(r(0.35, 0.54, 0.20, 0.035), 1.0, color);
            if kind == IconKind::Archive {
                painter.rect_stroke(r(0.45, 0.16, 0.10, 0.68), 1.0, stroke, StrokeKind::Middle);
            }
            if kind == IconKind::App {
                painter.rect_stroke(r(0.38, 0.60, 0.24, 0.14), 2.0, stroke, StrokeKind::Middle);
            }
        }
        IconKind::Image => {
            painter.rect_stroke(r(0.16, 0.22, 0.68, 0.56), 3.0, stroke, StrokeKind::Middle);
            painter.circle_filled(p(0.67, 0.38), rect.width() * 0.055, white);
            painter.add(egui::Shape::convex_polygon(
                vec![
                    p(0.22, 0.70),
                    p(0.42, 0.48),
                    p(0.54, 0.60),
                    p(0.66, 0.49),
                    p(0.79, 0.70),
                ],
                white,
                Stroke::NONE,
            ));
        }
        IconKind::Audio => {
            painter.line_segment([p(0.60, 0.24), p(0.60, 0.65)], stroke);
            painter.line_segment([p(0.60, 0.24), p(0.78, 0.20)], stroke);
            painter.line_segment([p(0.78, 0.20), p(0.78, 0.61)], stroke);
            painter.circle_filled(p(0.51, 0.69), rect.width() * 0.10, white);
            painter.circle_filled(p(0.69, 0.65), rect.width() * 0.10, white);
        }
        IconKind::Video => {
            painter.rect_stroke(r(0.16, 0.26, 0.68, 0.50), 4.0, stroke, StrokeKind::Middle);
            painter.add(egui::Shape::convex_polygon(
                vec![p(0.44, 0.38), p(0.44, 0.64), p(0.64, 0.51)],
                white,
                Stroke::NONE,
            ));
        }
        IconKind::Trash => {
            painter.rect_stroke(r(0.30, 0.33, 0.40, 0.48), 3.0, stroke, StrokeKind::Middle);
            painter.line_segment([p(0.25, 0.28), p(0.75, 0.28)], stroke);
            painter.line_segment([p(0.42, 0.20), p(0.58, 0.20)], stroke);
            painter.line_segment([p(0.44, 0.44), p(0.44, 0.70)], stroke);
            painter.line_segment([p(0.56, 0.44), p(0.56, 0.70)], stroke);
        }
    }
}

fn paint_nav_row(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    response: &egui::Response,
    kind: IconKind,
    label: &str,
    accent: egui::Color32,
) {
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, 8.0, ui.visuals().widgets.hovered.bg_fill);
    }
    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(rect.left() + 24.0, rect.center().y),
        egui::vec2(26.0, 26.0),
    );
    paint_icon(ui.painter(), icon_rect, kind, accent.gamma_multiply(0.82));
    ui.painter().text(
        egui::pos2(icon_rect.right() + 12.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(13.0),
        ui.visuals().text_color(),
    );
}

fn format_modified(time: Option<std::time::SystemTime>) -> String {
    time.map(|time| {
        DateTime::<Local>::from(time)
            .format("%d/%m/%Y %H:%M")
            .to_string()
    })
    .unwrap_or_else(|| "—".into())
}

fn accent_color(settings: &AppSettings) -> egui::Color32 {
    egui::Color32::from_rgb(settings.accent[0], settings.accent[1], settings.accent[2])
}

fn apply_theme(ctx: &egui::Context, settings: &AppSettings) {
    ctx.set_zoom_factor(settings.font_scale);
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = if settings.compact_rows {
        egui::vec2(6.0, 4.0)
    } else {
        egui::vec2(9.0, 8.0)
    };
    ctx.set_style(style);
    let dark = match settings.theme {
        ThemeChoice::Dark => true,
        ThemeChoice::Light => false,
        ThemeChoice::System => system_is_dark(),
    };
    let mut visuals = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    if dark {
        visuals.panel_fill = egui::Color32::from_rgb(18, 21, 28);
        visuals.window_fill = egui::Color32::from_rgb(25, 29, 38);
        visuals.extreme_bg_color = egui::Color32::from_rgb(14, 16, 22);
        visuals.faint_bg_color = egui::Color32::from_rgb(30, 35, 46);
        visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(35, 41, 53);
        visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(24, 28, 36);
    }
    visuals.selection.bg_fill = accent_color(settings);
    visuals.hyperlink_color = accent_color(settings);
    visuals.widgets.active.bg_fill = accent_color(settings);
    visuals.widgets.hovered.bg_fill = if dark {
        egui::Color32::from_rgb(48, 56, 74)
    } else {
        egui::Color32::from_rgb(225, 233, 250)
    };
    ctx.set_visuals(visuals);
}

#[cfg(windows)]
fn detect_system_is_dark() -> bool {
    use winreg::{RegKey, enums::HKEY_CURRENT_USER};
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize")
        .and_then(|key| key.get_value::<u32, _>("AppsUseLightTheme"))
        .map(|value| value == 0)
        .unwrap_or(true)
}

#[cfg(not(windows))]
fn detect_system_is_dark() -> bool {
    true
}

fn system_is_dark() -> bool {
    static SYSTEM_DARK: OnceLock<bool> = OnceLock::new();
    *SYSTEM_DARK.get_or_init(detect_system_is_dark)
}

fn main() -> eframe::Result {
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/northstar.png"))
        .expect("ícone do aplicativo inválido");
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Northstar Files")
            .with_icon(icon)
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([850.0, 600.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Northstar Files",
        options,
        Box::new(|cc| Ok(Box::new(ExplorerApp::new(cc)))),
    )
}
