mod auth;
mod instances;
mod minecraft;
mod modded;
mod packs;
mod runtime;
pub fn microsoft_client_id() -> String {
    std::env::var("MICROSOFT_CLIENT_ID")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| option_env!("MICROSOFT_CLIENT_ID").map(str::to_owned))
        .unwrap_or_default()
}
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, sync::Mutex};
use tauri::Manager;

pub type Result<T> = std::result::Result<T, String>;
pub fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub memory_mb: u32,
    #[serde(default)]
    pub instances_directory: String,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            memory_mb: 4096,
            instances_directory: String::new(),
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Instance {
    pub id: String,
    pub name: String,
    pub version: String,
    pub loader: String,
    pub status: String,
    pub mod_count: usize,
    #[serde(default)]
    pub icon_path: Option<String>,
    #[serde(default)]
    pub profile_id: Option<String>,
    #[serde(default)]
    pub directory: Option<String>,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Store {
    pub settings: Settings,
    pub instances: Vec<Instance>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryInfo {
    pub total_mb: u32,
}
pub struct AppState {
    pub root: PathBuf,
    pub store: Mutex<Store>,
    pub session: Mutex<Option<auth::Session>>,
    pub pending: Mutex<Option<auth::Pending>>,
    pub busy: tokio::sync::Mutex<()>,
}
pub fn instances_root(state: &AppState) -> PathBuf {
    let configured = state
        .store
        .lock()
        .ok()
        .map(|store| store.settings.instances_directory.clone())
        .unwrap_or_default();
    if configured.trim().is_empty() {
        state.root.join("instances")
    } else {
        PathBuf::from(configured)
    }
}
impl AppState {
    pub fn save(&self) -> Result<()> {
        let guard = self.store.lock().map_err(err)?;
        let bytes = serde_json::to_vec_pretty(&*guard).map_err(err)?;
        let tmp = self.root.join("state.tmp");
        std::fs::write(&tmp, bytes).map_err(err)?;
        std::fs::rename(tmp, self.root.join("state.json")).map_err(err)
    }
}
pub fn http() -> reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .user_agent("HXLauncher/0.1.0")
                .connect_timeout(std::time::Duration::from_secs(15))
                .timeout(std::time::Duration::from_secs(180))
                .build()
                .expect("HTTP client")
        })
        .clone()
}
#[tauri::command]
fn get_store(state: tauri::State<AppState>) -> Result<Store> {
    let mut store = state.store.lock().map_err(err)?.clone();
    for instance in &mut store.instances {
        instance.icon_path = instance.icon_path.as_ref().and_then(|path| {
            let bytes = std::fs::read(path).ok()?;
            if bytes.len() > 2 * 1024 * 1024 {
                return None;
            }
            Some(format!(
                "data:image/png;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(bytes)
            ))
        });
    }
    Ok(store)
}
#[tauri::command]
fn system_memory() -> MemoryInfo {
    let mut system = sysinfo::System::new();
    system.refresh_memory();
    let total_mb = (system.total_memory() / 1024 / 1024) as u32;
    MemoryInfo { total_mb }
}
#[tauri::command]
fn save_settings(settings: Settings, state: tauri::State<AppState>) -> Result<()> {
    let _guard = state
        .busy
        .try_lock()
        .map_err(|_| "Attends la fin de l’opération avant de modifier les paramètres.")?;
    if !(1024..=32768).contains(&settings.memory_mb) {
        return Err("Mémoire : entre 1 et 32 Go.".into());
    }
    if !settings.instances_directory.trim().is_empty() {
        std::fs::create_dir_all(&settings.instances_directory)
            .map_err(|e| format!("Impossible de créer le dossier des instances : {e}"))?;
    }
    state.store.lock().map_err(err)?.settings = settings;
    state.save()
}
#[tauri::command]
async fn create_instance(
    name: String,
    version: String,
    state: tauri::State<'_, AppState>,
) -> Result<Instance> {
    let _guard = state
        .busy
        .try_lock()
        .map_err(|_| "Une opération est déjà en cours.")?;
    if name.trim().is_empty() || name.len() > 80 {
        return Err("Choisis un nom de 1 à 80 caractères.".into());
    }
    if !minecraft::versions().await?.iter().any(|v| v.id == version) {
        return Err("Version Minecraft inconnue.".into());
    }
    let id = format!(
        "instance-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(err)?
            .as_millis()
    );
    let instance = Instance {
        directory: Some(instances::available_name(&state, &name)?),
        id,
        name: name.trim().into(),
        version,
        loader: "Vanilla".into(),
        status: "À installer".into(),
        mod_count: 0,
        icon_path: None,
        profile_id: None,
    };
    state
        .store
        .lock()
        .map_err(err)?
        .instances
        .push(instance.clone());
    state.save()?;
    Ok(instance)
}
#[tauri::command]
fn set_instance_icon(id: String, path: String, state: tauri::State<AppState>) -> Result<()> {
    let _guard = state
        .busy
        .try_lock()
        .map_err(|_| "Attends la fin de l’opération avant de modifier l’icône.")?;
    let source = std::path::PathBuf::from(path);
    if source.extension().and_then(|value| value.to_str()) != Some("png") {
        return Err("L’icône doit être un fichier PNG.".into());
    }
    let bytes = std::fs::read(&source).map_err(err)?;
    if bytes.len() > 2 * 1024 * 1024 || !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err("L’icône doit être un PNG valide de 2 Mo maximum.".into());
    }
    let target = instances::directory(&state, &id)?.join("icon.png");
    std::fs::create_dir_all(target.parent().ok_or("Dossier d’icône invalide.")?).map_err(err)?;
    std::fs::write(&target, bytes).map_err(err)?;
    let mut store = state.store.lock().map_err(err)?;
    let instance = store
        .instances
        .iter_mut()
        .find(|instance| instance.id == id)
        .ok_or("Instance introuvable.")?;
    instance.icon_path = Some(target.to_string_lossy().into());
    drop(store);
    state.save()
}
#[tauri::command]
fn delete_instance(id: String, state: tauri::State<AppState>) -> Result<()> {
    let _guard = state
        .busy
        .try_lock()
        .map_err(|_| "Attends la fin de l’opération avant de supprimer une instance.")?;
    let instance_path = instances::directory(&state, &id)?;
    if instance_path.exists() {
        std::fs::remove_dir_all(&instance_path)
            .map_err(|e| format!("Impossible de supprimer l’instance : {e}"))?;
    }
    let removed = state
        .store
        .lock()
        .map_err(err)?
        .instances
        .iter()
        .any(|instance| instance.id == id);
    if !removed {
        return Err("Instance introuvable.".into());
    }
    state
        .store
        .lock()
        .map_err(err)?
        .instances
        .retain(|instance| instance.id != id);
    state.save()
}
#[tauri::command]
fn data_directory(state: tauri::State<AppState>) -> String {
    state.root.to_string_lossy().into()
}
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            #[cfg(target_os = "macos")]
            {
                use objc2_app_kit::{NSWindow, NSWindowButton};
                let window = app
                    .get_webview_window("main")
                    .ok_or("Main window missing")?;
                // A titled full-content window keeps macOS's native rounded mask
                // while remaining opaque. Our HTML header supplies all controls.
                // SAFETY: Tauri setup runs on the main thread; the borrowed
                // NSWindow belongs to the live webview window for this scope.
                let native = unsafe { &*window.ns_window()?.cast::<NSWindow>() };
                for kind in [
                    NSWindowButton::CloseButton,
                    NSWindowButton::MiniaturizeButton,
                    NSWindowButton::ZoomButton,
                ] {
                    if let Some(button) = native.standardWindowButton(kind) {
                        button.setHidden(true);
                    }
                }
            }
            let root = app.path().app_data_dir()?;
            std::fs::create_dir_all(&root)?;
            let store = match std::fs::read(root.join("state.json")) {
                Ok(bytes) => serde_json::from_slice(&bytes)?,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Store::default(),
                Err(e) => return Err(e.into()),
            };
            app.manage(AppState {
                root,
                store: Mutex::new(store),
                session: Mutex::new(None),
                pending: Mutex::new(None),
                busy: tokio::sync::Mutex::new(()),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_store,
            system_memory,
            save_settings,
            create_instance,
            delete_instance,
            set_instance_icon,
            data_directory,
            auth::begin_login,
            auth::skin_texture,
            auth::poll_login,
            auth::restore_session,
            auth::logout,
            auth::cancel_login,
            minecraft::list_versions,
            minecraft::launch_instance,
            packs::inspect_modpack,
            packs::install_modpack
        ])
        .run(tauri::generate_context!())
        .expect("Unable to run HX Launcher");
}
