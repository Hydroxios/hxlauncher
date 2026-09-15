mod auth;
mod minecraft;
mod modded;
mod packs;
pub fn microsoft_client_id() -> String {
    std::env::var("MICROSOFT_CLIENT_ID")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| option_env!("MICROSOFT_CLIENT_ID").map(str::to_owned))
        .unwrap_or_default()
}
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
    pub java_path: String,
    pub memory_mb: u32,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            java_path: "java".into(),
            memory_mb: 4096,
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
    pub profile_id: Option<String>,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Store {
    pub settings: Settings,
    pub instances: Vec<Instance>,
}
pub struct AppState {
    pub root: PathBuf,
    pub store: Mutex<Store>,
    pub session: Mutex<Option<auth::Session>>,
    pub pending: Mutex<Option<auth::Pending>>,
    pub busy: tokio::sync::Mutex<()>,
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
    Ok(state.store.lock().map_err(err)?.clone())
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
    if settings.java_path.trim().is_empty() {
        return Err("Indique un exécutable Java.".into());
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
        id,
        name: name.trim().into(),
        version,
        loader: "Vanilla".into(),
        status: "À installer".into(),
        mod_count: 0,
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
async fn check_java(path: String) -> Result<String> {
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        tokio::process::Command::new(path).arg("-version").output(),
    )
    .await
    .map_err(|_| "Java ne répond pas.")?
    .map_err(|e| format!("Java introuvable : {e}"))?;
    if !result.status.success() {
        return Err(String::from_utf8_lossy(&result.stderr).into());
    }
    Ok(String::from_utf8_lossy(&result.stderr)
        .lines()
        .next()
        .unwrap_or("Java disponible")
        .into())
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
            save_settings,
            create_instance,
            check_java,
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
