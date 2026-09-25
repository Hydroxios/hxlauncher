use crate::{err, minecraft::safe_path, Result};
use serde::{Deserialize, Serialize};
use std::{io::Read, path::Path};
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModFile {
    #[serde(rename = "projectID")]
    pub project_id: u64,
    #[serde(rename = "fileID")]
    pub file_id: u64,
    #[serde(default = "required")]
    pub required: bool,
}
fn required() -> bool {
    true
}
#[derive(Deserialize)]
struct Loader {
    id: String,
    #[serde(default)]
    primary: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Minecraft {
    version: String,
    mod_loaders: Vec<Loader>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    name: String,
    version: String,
    #[serde(default)]
    author: String,
    manifest_type: String,
    manifest_version: u32,
    minecraft: Minecraft,
    files: Vec<ModFile>,
    #[serde(default)]
    overrides: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackPlan {
    name: String,
    version: String,
    author: String,
    minecraft: String,
    loader: String,
    files: Vec<ModFile>,
    override_count: usize,
    archive_path: String,
    #[serde(skip)]
    overrides: String,
}
fn inspect(path: &str) -> Result<PackPlan> {
    let file = std::fs::File::open(path).map_err(err)?;
    if file.metadata().map_err(err)?.len() > 512 * 1024 * 1024 {
        return Err("Le ZIP dépasse la limite de 512 Mo.".into());
    }
    let mut archive =
        zip::ZipArchive::new(file).map_err(|_| "Ce fichier n’est pas une archive ZIP valide.")?;
    if archive.len() > 50_000 {
        return Err("Trop de fichiers dans cette archive.".into());
    }
    let m: Manifest = {
        let f = archive.by_name("manifest.json").map_err(|_| {
            "manifest.json absent à la racine. Utilise un export CurseForge de modpack Java."
        })?;
        if f.size() > 8 * 1024 * 1024 {
            return Err("Manifeste trop volumineux.".into());
        }
        let mut text = String::new();
        f.take(8 * 1024 * 1024 + 1)
            .read_to_string(&mut text)
            .map_err(err)?;
        serde_json::from_str(&text).map_err(|e| format!("Manifeste CurseForge invalide : {e}"))?
    };
    if m.manifest_type != "minecraftModpack" || m.manifest_version != 1 {
        return Err("Format de modpack non pris en charge.".into());
    }
    if m.files.len() > 20_000 {
        return Err("Ce modpack contient trop de mods.".into());
    }
    let mut total = 0u64;
    let mut override_count = 0;
    let prefix = if m.overrides.is_empty() {
        None
    } else {
        safe_path(Path::new("/pack"), &m.overrides)?;
        Some(format!("{}/", m.overrides.trim_end_matches('/')))
    };
    for n in 0..archive.len() {
        let f = archive.by_index(n).map_err(err)?;
        safe_path(Path::new("/pack"), f.name().trim_end_matches('/'))?;
        if f.unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err("Les liens symboliques ne sont pas acceptés dans un modpack.".into());
        }
        total = total
            .checked_add(f.size())
            .ok_or("Archive trop volumineuse.")?;
        if total > 4 * 1024 * 1024 * 1024 {
            return Err("Le contenu décompressé dépasse 4 Go.".into());
        }
        if !f.is_dir() && prefix.as_ref().is_some_and(|p| f.name().starts_with(p)) {
            override_count += 1;
        }
    }
    let loader = m
        .minecraft
        .mod_loaders
        .iter()
        .find(|l| l.primary)
        .or(m.minecraft.mod_loaders.first())
        .map(|l| l.id.clone())
        .unwrap_or_else(|| "Vanilla".into());
    Ok(PackPlan {
        name: m.name,
        version: m.version,
        author: m.author,
        minecraft: m.minecraft.version,
        loader,
        files: m.files,
        override_count,
        archive_path: path.into(),
        overrides: m.overrides,
    })
}
#[tauri::command]
pub async fn inspect_modpack(path: String) -> Result<PackPlan> {
    tauri::async_runtime::spawn_blocking(move || inspect(&path))
        .await
        .map_err(err)?
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    fn fixture(extra: &str) -> String {
        let path = std::env::temp_dir().join(format!(
            "hx-test-{}-{}.zip",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut z = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        z.start_file("manifest.json", opts).unwrap();
        z.write_all(br#"{"name":"Test","version":"1","manifestType":"minecraftModpack","manifestVersion":1,"minecraft":{"version":"1.21.1","modLoaders":[{"id":"forge-52.0.1","primary":true}]},"files":[{"projectID":1,"fileID":2,"required":true}],"overrides":"overrides"}"#).unwrap();
        z.start_file(extra, opts).unwrap();
        z.write_all(b"test").unwrap();
        z.finish().unwrap();
        path.to_string_lossy().into()
    }
    #[test]
    fn reads_curseforge_export() {
        let p = fixture("overrides/config/test.toml");
        let result = inspect(&p);
        std::fs::remove_file(p).unwrap();
        let plan = result.unwrap();
        assert_eq!(plan.override_count, 1);
        assert_eq!(plan.files[0].project_id, 1);
        assert_eq!(plan.loader, "forge-52.0.1");
    }
    #[test]
    fn rejects_zip_traversal() {
        let p = fixture("../evil");
        assert!(inspect(&p).is_err());
        std::fs::remove_file(p).unwrap();
    }
}

fn extract(path: &Path, game: &Path, overrides: &str) -> Result<()> {
    if overrides.is_empty() {
        return Ok(());
    }
    let mut archive = zip::ZipArchive::new(std::fs::File::open(path).map_err(err)?).map_err(err)?;
    let prefix = format!("{}/", overrides.trim_end_matches('/'));
    let mut names = std::collections::HashSet::new();
    let mut remaining = 4 * 1024 * 1024 * 1024u64;
    for i in 0..archive.len() {
        let f = archive.by_index(i).map_err(err)?;
        if f.is_dir() {
            continue;
        }
        let Some(relative) = f.name().strip_prefix(&prefix) else {
            continue;
        };
        let target = safe_path(game, relative)?;
        if !names.insert(relative.to_lowercase()) || target.exists() {
            return Err(format!("Fichier en double dans le pack : {relative}"));
        }
        if f.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000) {
            return Err("Lien symbolique interdit.".into());
        }
        std::fs::create_dir_all(target.parent().ok_or("Chemin invalide.")?).map_err(err)?;
        let mut out = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
            .map_err(err)?;
        let count = std::io::copy(&mut f.take(remaining + 1), &mut out).map_err(err)?;
        remaining = remaining
            .checked_sub(count)
            .ok_or("Le contenu décompressé dépasse 4 Go.")?;
    }
    Ok(())
}
fn extract_icon(path: &Path, game: &Path) -> Result<bool> {
    let mut archive = zip::ZipArchive::new(std::fs::File::open(path).map_err(err)?).map_err(err)?;
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).map_err(err)?;
        let name = file.name().trim_end_matches('/');
        if !name.eq_ignore_ascii_case("icon.png")
            && !name.to_ascii_lowercase().ends_with("/icon.png")
        {
            continue;
        }
        if file.size() > 2 * 1024 * 1024 {
            return Err("L’icône du modpack dépasse 2 Mo.".into());
        }
        let mut bytes = Vec::with_capacity(file.size() as usize);
        file.read_to_end(&mut bytes).map_err(err)?;
        if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            return Err("L’icône du modpack n’est pas un PNG valide.".into());
        }
        let target = game.join("icon.png");
        std::fs::write(target, bytes).map_err(err)?;
        return Ok(true);
    }
    Ok(false)
}
fn api_key() -> Result<String> {
    std::env::var("CURSEFORGE_API_KEY")
        .ok()
        .filter(|k| !k.trim().is_empty())
        .or_else(|| option_env!("CURSEFORGE_API_KEY").map(str::to_owned))
        .filter(|k| !k.trim().is_empty())
        .ok_or_else(|| "CURSEFORGE_API_KEY absente de la configuration du launcher.".into())
}
async fn api(client: &reqwest::Client, key: &str, route: &str) -> Result<serde_json::Value> {
    let response = client
        .get(format!("https://api.curseforge.com/v1/{route}"))
        .header("accept", "application/json")
        .header("x-api-key", key)
        .send()
        .await
        .map_err(|_| "CurseForge est injoignable.")?;
    if !response.status().is_success() {
        let status = response.status().as_u16();
        let body = response.text().await.unwrap_or_default();
        let detail = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| {
                v.get("errorMessage")
                    .or_else(|| v.get("message"))
                    .and_then(|m| m.as_str())
                    .map(str::to_owned)
            })
            .unwrap_or_default();
        return Err(if status == 401 || status == 403 {
            if detail.is_empty() {
                format!("CurseForge refuse l’accès (HTTP {status}). Vérifie les droits de la clé API et sa configuration. Dans .env, entoure sa valeur de guillemets simples pour préserver les caractères $, puis reconstruis le launcher.")
            } else {
                format!("CurseForge refuse CURSEFORGE_API_KEY (HTTP {status}) : {detail}")
            }
        } else {
            format!("CurseForge : erreur HTTP {status}. Réessaie plus tard.")
        });
    }
    let value: serde_json::Value = response
        .json()
        .await
        .map_err(|_| "Réponse CurseForge invalide.")?;
    Ok(value["data"].clone())
}
fn mod_target(game: &Path, filename: &str, class_id: u64) -> Result<std::path::PathBuf> {
    if filename.contains('/') || filename.contains('\\') {
        return Err("Nom de mod invalide.".into());
    }
    let dir = match class_id {
        6 => "mods",
        12 => "resourcepacks",
        6552 => "shaderpacks",
        _ => {
            return Err(format!(
                "Type de contenu CurseForge non pris en charge : {class_id}"
            ))
        }
    };
    safe_path(&game.join(dir), filename)
}
async fn install_contents(
    plan: &PackPlan,
    stage: &Path,
    app: &tauri::AppHandle,
    state: &crate::AppState,
) -> Result<crate::Instance> {
    use crate::minecraft::{download, progress};
    let game = stage.join("game");
    tokio::fs::create_dir_all(&game).await.map_err(err)?;
    let key = if plan.files.is_empty() {
        String::new()
    } else {
        api_key()?
    };
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(err)?;
    let mut targets = std::collections::HashSet::new();
    for (i, file) in plan.files.iter().enumerate() {
        progress(
            app,
            "download",
            format!("Téléchargement des mods · {}/{}", i + 1, plan.files.len()),
            i,
            plan.files.len(),
        );
        let data = api(
            &client,
            &key,
            &format!("mods/{}/files/{}", file.project_id, file.file_id),
        )
        .await?;
        if data["id"].as_u64() != Some(file.file_id)
            || data["modId"].as_u64() != Some(file.project_id)
        {
            return Err("Référence CurseForge incohérente.".into());
        }
        let project = api(&client, &key, &format!("mods/{}", file.project_id)).await?;
        if project["gameId"].as_u64() != Some(432) {
            return Err("Ce fichier n’est pas destiné à Minecraft.".into());
        }
        let filename = data["fileName"].as_str().ok_or("Nom de mod absent.")?;
        let target = mod_target(
            &game,
            filename,
            project["classId"].as_u64().ok_or("Type de mod absent.")?,
        )?;
        if !targets.insert(target.to_string_lossy().to_lowercase()) {
            return Err(format!("Mod en double : {filename}"));
        }
        let url = data["downloadUrl"].as_str().filter(|u| !u.is_empty()).ok_or_else(|| format!("{filename} : l’auteur n’autorise pas le téléchargement via CurseForge. L’installation a été arrêtée."))?;
        let hash = data["hashes"]
            .as_array()
            .and_then(|a| a.iter().find(|h| h["algo"] == 1))
            .and_then(|h| h["value"].as_str())
            .filter(|h| h.len() == 40 && h.bytes().all(|c| c.is_ascii_hexdigit()))
            .ok_or_else(|| format!("Empreinte SHA-1 absente pour {filename}."))?
            .to_lowercase();
        download(url, &target, Some(&hash), 512 * 1024 * 1024).await?;
    }
    progress(app, "prepare", "Extraction des configurations…", 0, 0);
    let (archive, g, overrides) = (stage.join("pack.zip"), game.clone(), plan.overrides.clone());
    tauri::async_runtime::spawn_blocking(move || extract(&archive, &g, &overrides))
        .await
        .map_err(err)??;
    let has_icon = extract_icon(&stage.join("pack.zip"), &game)?;
    let profile_id = crate::modded::install(
        state.root.join("modded-runtime"),
        plan.minecraft.clone(),
        plan.loader.clone(),
        app.clone(),
    )
    .await?;
    Ok(crate::Instance {
        id: stage
            .file_name()
            .ok_or("Identifiant absent.")?
            .to_string_lossy()
            .into(),
        name: plan.name.clone(),
        version: plan.minecraft.clone(),
        loader: plan.loader.clone(),
        status: "Installé".into(),
        mod_count: plan.files.len(),
        icon_path: has_icon.then(|| game.join("icon.png").to_string_lossy().into()),
        profile_id: Some(profile_id),
        directory: None,
    })
}
#[tauri::command]
pub async fn install_modpack(
    path: String,
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::AppState>,
) -> Result<crate::Instance> {
    let _guard = state
        .busy
        .try_lock()
        .map_err(|_| "Un jeu ou une installation est déjà en cours.")?;
    let id = format!(
        "pack-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(err)?
            .as_nanos()
    );
    let stage = state.root.join("imports").join(&id);
    tokio::fs::create_dir_all(&stage).await.map_err(err)?;
    let result = async {
        // Work from a private snapshot; never trust a previously inspected archive.
        let mut source = tokio::fs::File::open(&path).await.map_err(err)?;
        if source.metadata().await.map_err(err)?.len() > 512 * 1024 * 1024 {
            return Err("Le ZIP dépasse 512 Mo.".into());
        }
        let archive = stage.join("pack.zip");
        let mut dest = tokio::fs::File::create(&archive).await.map_err(err)?;
        use tokio::io::AsyncReadExt;
        let copied = tokio::io::copy(&mut (&mut source).take(512 * 1024 * 1024 + 1), &mut dest)
            .await
            .map_err(err)?;
        if copied > 512 * 1024 * 1024 {
            return Err("Le ZIP dépasse 512 Mo.".into());
        }
        drop(dest);
        let plan = inspect_modpack(archive.to_string_lossy().into()).await?;
        crate::modded::identifier(&plan.minecraft)?;
        let mut instance = install_contents(&plan, &stage, &app, &state).await?;
        let directory = crate::instances::available_name(&state, &instance.name)?;
        let destination = safe_path(&crate::instances_root(&state), &directory)?;
        instance.directory = Some(directory);
        tokio::fs::create_dir_all(destination.parent().unwrap())
            .await
            .map_err(err)?;
        tokio::fs::rename(stage.join("game"), &destination)
            .await
            .map_err(err)?;
        if instance.icon_path.is_some() {
            instance.icon_path = Some(destination.join("icon.png").to_string_lossy().into());
        }
        state
            .store
            .lock()
            .map_err(err)?
            .instances
            .push(instance.clone());
        if let Err(e) = state.save() {
            state
                .store
                .lock()
                .map_err(err)?
                .instances
                .retain(|i| i.id != id);
            let _ = tokio::fs::remove_dir_all(destination).await;
            return Err(e);
        }
        Ok(instance)
    }
    .await;
    let _ = tokio::fs::remove_dir_all(&stage).await;
    match &result {
        Ok(_) => crate::minecraft::progress(&app, "idle", "Modpack installé.", 1, 1),
        Err(e) => crate::minecraft::progress(&app, "error", e, 0, 0),
    }
    result
}

#[cfg(test)]
mod install_tests {
    use super::*;
    use std::io::Write;
    #[test]
    fn routes_content_and_rejects_filenames() {
        assert_eq!(
            mod_target(Path::new("game"), "a.jar", 6).unwrap(),
            Path::new("game/mods/a.jar")
        );
        assert_eq!(
            mod_target(Path::new("game"), "a.zip", 12).unwrap(),
            Path::new("game/resourcepacks/a.zip")
        );
        assert_eq!(
            mod_target(Path::new("game"), "a.zip", 6552).unwrap(),
            Path::new("game/shaderpacks/a.zip")
        );
        for filename in ["../a.jar", "/a.jar", "a\\b.jar", "C:a.jar", ".."] {
            assert!(mod_target(Path::new("game"), filename, 6).is_err());
        }
        assert!(mod_target(Path::new("game"), "a.jar", 4471).is_err());
    }
    #[test]
    fn extracts_only_overrides_and_preserves_existing_mods() {
        let root = std::env::temp_dir().join(format!(
            "hx-extract-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("pack.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        for (name, bytes) in [
            ("manifest.json", "manifest"),
            ("overrides/config/a.txt", "config"),
            ("overrides/mods/a.jar", "untrusted replacement"),
        ] {
            zip.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(bytes.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
        let game = root.join("game");
        extract(&path, &game, "overrides").unwrap();
        assert_eq!(
            std::fs::read_to_string(game.join("config/a.txt")).unwrap(),
            "config"
        );
        assert!(!game.join("manifest.json").exists());
        std::fs::write(game.join("mods/a.jar"), "verified mod").unwrap();
        assert!(extract(&path, &game, "overrides").is_err());
        assert_eq!(
            std::fs::read_to_string(game.join("mods/a.jar")).unwrap(),
            "verified mod"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
