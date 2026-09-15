use crate::{auth, err, http, AppState, Result};
use futures_util::{stream, StreamExt, TryStreamExt};
use serde::Serialize;
use serde_json::Value;
use sha1::{Digest, Sha1};
use std::{
    collections::HashMap,
    path::{Component, Path, PathBuf},
    process::Stdio,
};
use tauri::Emitter;
#[derive(Clone, Serialize)]
pub struct Version {
    pub id: String,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Progress {
    phase: String,
    message: String,
    current: usize,
    total: usize,
}
pub(crate) fn progress(
    app: &tauri::AppHandle,
    phase: &str,
    message: impl Into<String>,
    current: usize,
    total: usize,
) {
    let _ = app.emit(
        "launcher-progress",
        Progress {
            phase: phase.into(),
            message: message.into(),
            current,
            total,
        },
    );
}
pub fn safe_path(root: &Path, relative: &str) -> Result<PathBuf> {
    if relative.is_empty()
        || relative.contains('\\')
        || relative.contains(':')
        || Path::new(relative)
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err("Chemin de fichier non sûr dans le manifeste.".into());
    }
    Ok(root.join(relative))
}
async fn json(url: &str) -> Result<Value> {
    http()
        .get(url)
        .send()
        .await
        .map_err(err)?
        .error_for_status()
        .map_err(err)?
        .json()
        .await
        .map_err(err)
}
async fn manifest() -> Result<Value> {
    json("https://piston-meta.mojang.com/mc/game/version_manifest_v2.json").await
}
pub async fn versions() -> Result<Vec<Version>> {
    let m = manifest().await?;
    Ok(m["versions"]
        .as_array()
        .ok_or("Manifeste Mojang invalide.")?
        .iter()
        .filter(|v| {
            v["type"] == "release" && v["releaseTime"].as_str().unwrap_or("") >= "2022-06-07"
        })
        .filter_map(|v| v["id"].as_str().map(|id| Version { id: id.into() }))
        .collect())
}
#[tauri::command]
pub async fn list_versions() -> Result<Vec<Version>> {
    versions().await
}
pub(crate) async fn download(
    url: &str,
    path: &Path,
    hash: Option<&str>,
    limit: usize,
) -> Result<()> {
    if let Ok(bytes) = tokio::fs::read(path).await {
        if hash.is_some_and(|h| format!("{:x}", Sha1::digest(&bytes)) == h) {
            return Ok(());
        }
    }
    if !url.starts_with("https://") {
        return Err("Téléchargement non HTTPS refusé.".into());
    }
    let res = http()
        .get(url)
        .send()
        .await
        .map_err(err)?
        .error_for_status()
        .map_err(err)?;
    if res.content_length().is_some_and(|n| n > limit as u64) {
        return Err("Fichier trop volumineux.".into());
    }
    let mut stream = res.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(err)?;
        if bytes.len() + chunk.len() > limit {
            return Err("Fichier trop volumineux.".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    if let Some(hash) = hash {
        if format!("{:x}", Sha1::digest(&bytes)) != hash {
            return Err(format!(
                "Intégrité SHA-1 incorrecte : {}",
                path.file_name().unwrap_or_default().to_string_lossy()
            ));
        }
    }
    tokio::fs::create_dir_all(path.parent().ok_or("Dossier invalide.")?)
        .await
        .map_err(err)?;
    let tmp = path.with_extension("hx-download");
    tokio::fs::write(&tmp, bytes).await.map_err(err)?;
    tokio::fs::rename(tmp, path).await.map_err(err)
}
fn os() -> &'static str {
    if cfg!(target_os = "macos") {
        "osx"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "linux"
    }
}
fn allowed(value: &Value) -> bool {
    let Some(rules) = value["rules"].as_array() else {
        return true;
    };
    let mut allow = false;
    for rule in rules {
        let platform = &rule["os"];
        let name_match = platform["name"].as_str().is_none_or(|n| n == os());
        let arch_match = platform["arch"].as_str().is_none_or(|a| {
            a == std::env::consts::ARCH || (a == "arm64" && cfg!(target_arch = "aarch64"))
        });
        // Version-specific OS rules cannot be safely assumed to match.
        let version_match = platform.get("version").is_none();
        let features_match = rule["features"]
            .as_object()
            .is_none_or(|f| f.values().all(|v| v == false));
        if name_match && arch_match && version_match && features_match {
            allow = rule["action"] == "allow";
        }
    }
    allow
}
fn arguments(list: &Value, vars: &HashMap<&str, String>) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for item in list
        .as_array()
        .ok_or("Cette version utilise un format d’arguments non pris en charge.")?
    {
        let values: Vec<&str> = if let Some(s) = item.as_str() {
            vec![s]
        } else if allowed(item) {
            if let Some(s) = item["value"].as_str() {
                vec![s]
            } else {
                item["value"]
                    .as_array()
                    .map(|a| a.iter().filter_map(Value::as_str).collect())
                    .unwrap_or_default()
            }
        } else {
            vec![]
        };
        for value in values {
            let mut value = value.to_string();
            for (key, replacement) in vars {
                value = value.replace(&format!("${{{key}}}"), replacement);
            }
            if value.contains("${") {
                return Err(
                    "Cette version demande un argument de lancement non pris en charge.".into(),
                );
            }
            out.push(value);
        }
    }
    Ok(out)
}
async fn artifact(value: &Value, root: &Path) -> Result<PathBuf> {
    let path = safe_path(
        root,
        value["path"]
            .as_str()
            .ok_or("Chemin de bibliothèque absent.")?,
    )?;
    download(
        value["url"]
            .as_str()
            .ok_or("URL de bibliothèque absente.")?,
        &path,
        value["sha1"].as_str(),
        256 * 1024 * 1024,
    )
    .await?;
    Ok(path)
}
fn extract_natives(jar: &Path, dest: &Path) -> Result<()> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(jar).map_err(err)?).map_err(err)?;
    let mut total = 0u64;
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).map_err(err)?;
        if f.is_dir() || f.name().starts_with("META-INF/") {
            continue;
        }
        total += f.size();
        if total > 256 * 1024 * 1024 {
            return Err("Archive native trop volumineuse.".into());
        }
        let target = safe_path(dest, f.name())?;
        std::fs::create_dir_all(target.parent().ok_or("Chemin natif invalide.")?).map_err(err)?;
        let mut out = std::fs::File::create(target).map_err(err)?;
        std::io::copy(&mut f, &mut out).map_err(err)?;
    }
    Ok(())
}
#[tauri::command]
pub async fn launch_instance(
    id: String,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<()> {
    let _guard = state
        .busy
        .try_lock()
        .map_err(|_| "Un jeu ou une installation est déjà en cours.")?;
    let result = launch(&id, &app, &state).await;
    if let Err(ref e) = result {
        progress(&app, "error", e, 0, 0);
    }
    result
}
async fn launch(id: &str, app: &tauri::AppHandle, state: &AppState) -> Result<()> {
    let (instance, settings) = {
        let s = state.store.lock().map_err(err)?;
        (
            s.instances
                .iter()
                .find(|i| i.id == id)
                .cloned()
                .ok_or("Instance introuvable.")?,
            s.settings.clone(),
        )
    };
    if instance.profile_id.is_some() {
        return crate::modded::launch(instance, settings, app, state).await;
    }
    if instance.loader != "Vanilla" {
        return Err(
            "L’installation des modloaders sera ajoutée dans une prochaine version.".into(),
        );
    }
    progress(app, "prepare", "Vérification de Java et du compte…", 0, 0);
    let java = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        tokio::process::Command::new(&settings.java_path)
            .arg("-version")
            .output(),
    )
    .await
    .map_err(|_| "Java ne répond pas.")?
    .map_err(|e| format!("Installe Java puis indique son chemin dans les paramètres : {e}"))?;
    if !java.status.success() {
        return Err("Java ne démarre pas. Vérifie le chemin dans les paramètres.".into());
    }
    let session = auth::refresh(state)
        .await?
        .ok_or("Connecte-toi avec Microsoft avant de jouer.")?;
    *state.session.lock().map_err(err)? = Some(session.clone());
    let manifest = manifest().await?;
    let entry = manifest["versions"]
        .as_array()
        .ok_or("Manifeste invalide.")?
        .iter()
        .find(|v| v["id"] == instance.version)
        .ok_or("Version introuvable chez Mojang.")?;
    let game = safe_path(&state.root.join("instances"), id)?;
    tokio::fs::create_dir_all(&game).await.map_err(err)?;
    let shared = state.root.join("minecraft");
    let version_dir = safe_path(&shared.join("versions"), &instance.version)?;
    let metadata_path = version_dir.join("version.json");
    download(
        entry["url"].as_str().ok_or("URL manquante.")?,
        &metadata_path,
        entry["sha1"].as_str(),
        8 * 1024 * 1024,
    )
    .await?;
    let meta: Value =
        serde_json::from_slice(&tokio::fs::read(metadata_path).await.map_err(err)?).map_err(err)?;
    let required_java = meta["javaVersion"]["majorVersion"].as_u64().unwrap_or(17);
    let java_text = format!(
        "{}{}",
        String::from_utf8_lossy(&java.stderr),
        String::from_utf8_lossy(&java.stdout)
    );
    let actual = regex::Regex::new(r#"version "(\d+)"#)
        .map_err(err)?
        .captures(&java_text)
        .and_then(|c| c.get(1))
        .and_then(|v| v.as_str().parse::<u64>().ok());
    if actual.is_none_or(|v| v < required_java) {
        return Err(format!("Minecraft {} nécessite Java {required_java} ou supérieur. Configure le bon exécutable dans les paramètres.",instance.version));
    }
    let client = version_dir.join("client.jar");
    let d = &meta["downloads"]["client"];
    progress(app, "download", "Téléchargement de Minecraft…", 0, 0);
    download(
        d["url"].as_str().ok_or("Client absent.")?,
        &client,
        d["sha1"].as_str(),
        256 * 1024 * 1024,
    )
    .await?;
    let libraries = shared.join("libraries");
    let natives = game.join("natives");
    tokio::fs::create_dir_all(&natives).await.map_err(err)?;
    let libs = meta["libraries"]
        .as_array()
        .ok_or("Bibliothèques absentes.")?;
    let mut classpath = Vec::new();
    for (i, lib) in libs.iter().enumerate() {
        if !allowed(lib) {
            continue;
        }
        progress(
            app,
            "download",
            "Installation des bibliothèques…",
            i,
            libs.len(),
        );
        if lib["downloads"].get("artifact").is_some() {
            classpath.push(artifact(&lib["downloads"]["artifact"], &libraries).await?);
        }
        if let Some(classifier) = lib["natives"][os()].as_str() {
            if cfg!(target_arch = "aarch64") {
                return Err("Cette version utilise des bibliothèques natives x86. Choisis une version récente compatible ARM.".into());
            }
            let classifier = classifier.replace(
                "${arch}",
                if cfg!(target_pointer_width = "64") {
                    "64"
                } else {
                    "32"
                },
            );
            let jar = artifact(&lib["downloads"]["classifiers"][&classifier], &libraries).await?;
            extract_natives(&jar, &natives)?;
        }
    }
    classpath.push(client);
    let assets = shared.join("assets");
    let index_id = meta["assetIndex"]["id"]
        .as_str()
        .ok_or("Index d’assets absent.")?;
    let index_path = safe_path(&assets.join("indexes"), &format!("{index_id}.json"))?;
    download(
        meta["assetIndex"]["url"]
            .as_str()
            .ok_or("URL d’assets absente.")?,
        &index_path,
        meta["assetIndex"]["sha1"].as_str(),
        32 * 1024 * 1024,
    )
    .await?;
    let index: Value =
        serde_json::from_slice(&tokio::fs::read(index_path).await.map_err(err)?).map_err(err)?;
    let objects = index["objects"]
        .as_object()
        .ok_or("Index d’assets invalide.")?;
    let mut hashes: Vec<String> = objects
        .values()
        .map(|o| o["hash"].as_str().unwrap_or("").to_owned())
        .collect();
    hashes.sort();
    hashes.dedup();
    let total = hashes.len();
    let completed = std::sync::atomic::AtomicUsize::new(0);
    stream::iter(hashes)
        .map(|hash| {
            let assets = assets.clone();
            let completed = &completed;
            async move {
                if hash.len() != 40 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return Err("Hash d’asset invalide.".into());
                }
                let rel = format!("{}/{}", &hash[..2], hash);
                let path = assets.join("objects").join(&rel);
                download(
                    &format!("https://resources.download.minecraft.net/{rel}"),
                    &path,
                    Some(&hash),
                    64 * 1024 * 1024,
                )
                .await?;
                let count = completed.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                if count.is_multiple_of(25) || count == total {
                    progress(
                        app,
                        "download",
                        "Synchronisation des ressources…",
                        count,
                        total,
                    );
                }
                Ok::<(), String>(())
            }
        })
        .buffer_unordered(12)
        .try_collect::<Vec<_>>()
        .await?;
    let cp = std::env::join_paths(&classpath)
        .map_err(err)?
        .to_string_lossy()
        .into_owned();
    let mut vars = HashMap::new();
    for (k, v) in [
        ("auth_player_name", session.profile.name),
        ("auth_uuid", session.profile.id),
        ("auth_access_token", session.token),
        ("version_name", instance.version.clone()),
        ("game_directory", game.to_string_lossy().into()),
        ("assets_root", assets.to_string_lossy().into()),
        ("assets_index_name", index_id.into()),
        ("user_type", "msa".into()),
        ("version_type", "release".into()),
        ("natives_directory", natives.to_string_lossy().into()),
        ("launcher_name", "HXLauncher".into()),
        ("launcher_version", "0.1.0".into()),
        ("classpath", cp),
        ("clientid", crate::microsoft_client_id()),
        ("auth_xuid", String::new()),
        ("user_properties", "{}".into()),
        ("library_directory", libraries.to_string_lossy().into()),
        (
            "classpath_separator",
            if cfg!(windows) { ";" } else { ":" }.into(),
        ),
    ] {
        vars.insert(k, v);
    }
    let mut args = arguments(&meta["arguments"]["jvm"], &vars)?;
    args.push(format!("-Xmx{}M", settings.memory_mb));
    args.push("-Xms512M".into());
    args.push(
        meta["mainClass"]
            .as_str()
            .ok_or("Classe principale absente.")?
            .into(),
    );
    args.extend(arguments(&meta["arguments"]["game"], &vars)?);
    // Never write the command line or the access token to launcher logs.
    let log = std::fs::File::create(game.join("launcher-game.log")).map_err(err)?;
    let mut child = tokio::process::Command::new(&settings.java_path)
        .args(args)
        .current_dir(&game)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log.try_clone().map_err(err)?))
        .stderr(Stdio::from(log))
        .spawn()
        .map_err(|e| format!("Impossible de lancer Minecraft : {e}"))?;
    {
        let mut s = state.store.lock().map_err(err)?;
        if let Some(i) = s.instances.iter_mut().find(|i| i.id == id) {
            i.status = "Installé".into();
        }
    }
    state.save()?;
    progress(app, "running", "Minecraft est en cours d’exécution", 1, 1);
    let status = child.wait().await.map_err(err)?;
    if !status.success() {
        return Err(format!(
            "Minecraft s’est arrêté avec le code {}. Consulte {}.",
            status.code().unwrap_or(-1),
            game.join("launcher-game.log").display()
        ));
    }
    progress(
        app,
        "idle",
        "Minecraft est fermé. À la prochaine aventure.",
        0,
        0,
    );
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_escape_paths() {
        for p in [
            "../evil",
            "/tmp/evil",
            "a/../../x",
            "C:\\evil",
            "a\\..\\evil",
            "",
        ] {
            assert!(safe_path(Path::new("/safe"), p).is_err(), "{p}");
        }
        assert!(safe_path(Path::new("/safe"), "mods/mod.jar").is_ok());
    }
    #[test]
    fn rules_and_features() {
        assert!(!allowed(
            &serde_json::json!({"rules":[{"action":"allow","features":{"is_demo_user":true}}]})
        ));
        assert!(allowed(
            &serde_json::json!({"rules":[{"action":"allow","os":{"name":os()}}]})
        ));
        assert!(!allowed(
            &serde_json::json!({"rules":[{"action":"allow"},{"action":"disallow","os":{"name":os()}}]})
        ));
    }
    #[test]
    fn argument_expansion() {
        let vars = HashMap::from([("token", "secret".into())]);
        assert_eq!(arguments(&serde_json::json!(["--accessToken","${token}",{"rules":[{"action":"allow","features":{"is_demo_user":true}}],"value":"--demo"}]),&vars).unwrap(),vec!["--accessToken","secret"]);
        assert!(arguments(&serde_json::json!(["${missing}"]), &vars).is_err());
    }
}
