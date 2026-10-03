//! Skin editor backend: Minecraft Services texture changes and a local skin
//! library. PNGs cross the Tauri boundary as data URLs; tokens stay in Rust.
use crate::{auth, err, http, AppState, Result};
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const PROFILE_API: &str = "https://api.minecraftservices.com/minecraft/profile";
const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
const MAX_SKIN_BYTES: usize = 256 * 1024;
const DATA_URL_PREFIX: &str = "data:image/png;base64,";

/// Mojang only accepts 64 × 64 skins and legacy 64 × 32 ones.
fn validate_skin(bytes: &[u8]) -> Result<()> {
    if bytes.len() > MAX_SKIN_BYTES {
        return Err("Skin trop volumineux (256 Ko maximum).".into());
    }
    // IHDR is always the first chunk; width and height are big-endian u32s.
    if bytes.len() < 24 || !bytes.starts_with(PNG_SIGNATURE) || &bytes[12..16] != b"IHDR" {
        return Err("Le skin doit être une image PNG.".into());
    }
    let width = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    let height = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    if width != 64 || !(height == 64 || height == 32) {
        return Err("Le skin doit mesurer 64 × 64 ou 64 × 32 pixels.".into());
    }
    Ok(())
}

fn decode_skin(data: &str) -> Result<Vec<u8>> {
    let data = data.trim();
    let encoded = data.strip_prefix(DATA_URL_PREFIX).unwrap_or(data);
    if encoded.len() > MAX_SKIN_BYTES * 4 / 3 + 4 {
        return Err("Skin trop volumineux (256 Ko maximum).".into());
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| "Image de skin invalide.")?;
    validate_skin(&bytes)?;
    Ok(bytes)
}

fn data_url(bytes: &[u8]) -> String {
    format!(
        "{DATA_URL_PREFIX}{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}

fn variant(value: &str) -> Result<&'static str> {
    match value.trim().to_ascii_lowercase().as_str() {
        "classic" | "default" => Ok("classic"),
        "slim" => Ok("slim"),
        _ => Err("Modèle de skin inconnu.".into()),
    }
}

fn now_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn multipart(boundary: &str, variant: &str, png: &[u8]) -> Vec<u8> {
    let mut body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"variant\"\r\n\r\n{variant}\r\n\
         --{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"skin.png\"\r\n\
         Content-Type: image/png\r\n\r\n"
    )
    .into_bytes();
    body.extend_from_slice(png);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    body
}

async fn signed_in(state: &AppState) -> Result<auth::Session> {
    let session = auth::refresh(state)
        .await?
        .ok_or("Connecte-toi avec Microsoft pour modifier ton apparence.")?;
    *state.session.lock().map_err(err)? = Some(session.clone());
    Ok(session)
}

fn service_error(status: reqwest::StatusCode, subject: &str) -> String {
    match status.as_u16() {
        400 => format!("Minecraft Services refuse ce {subject}."),
        401 => "Session Minecraft expirée. Reconnecte-toi.".into(),
        403 => format!("Ce compte ne peut pas modifier ce {subject}."),
        404 => format!("{subject} introuvable sur ce compte."),
        429 => "Trop de changements en peu de temps. Réessaie dans une minute.".into(),
        _ => format!("Minecraft Services : HTTP {status}."),
    }
}

/// Mojang answers texture changes with the updated profile. Fall back to a
/// fresh profile request if the body ever stops matching.
async fn updated_profile(
    state: &AppState,
    token: &str,
    response: reqwest::Response,
    subject: &str,
) -> Result<auth::Profile> {
    let status = response.status();
    if !status.is_success() {
        return Err(service_error(status, subject));
    }
    let profile = match response.json::<auth::Profile>().await {
        Ok(profile) => profile,
        Err(_) => http()
            .get(PROFILE_API)
            .bearer_auth(token)
            .send()
            .await
            .map_err(err)?
            .error_for_status()
            .map_err(err)?
            .json()
            .await
            .map_err(err)?,
    };
    auth::replace_profile(state, profile.clone())?;
    Ok(profile)
}

/// Cached sessions may predate fields such as cape ids; refresh them on demand.
#[tauri::command]
pub async fn reload_profile(state: tauri::State<'_, AppState>) -> Result<auth::Profile> {
    let session = signed_in(&state).await?;
    let response = http()
        .get(PROFILE_API)
        .bearer_auth(&session.token)
        .send()
        .await
        .map_err(err)?;
    if !response.status().is_success() {
        return Err(service_error(response.status(), "profil"));
    }
    let profile: auth::Profile = response.json().await.map_err(err)?;
    auth::replace_profile(&state, profile.clone())?;
    Ok(profile)
}

#[tauri::command]
pub async fn upload_skin(
    data: String,
    variant: String,
    state: tauri::State<'_, AppState>,
) -> Result<auth::Profile> {
    let png = decode_skin(&data)?;
    let variant = self::variant(&variant)?;
    let session = signed_in(&state).await?;
    let boundary = format!("hxlauncher-{}", now_millis());
    let response = http()
        .post(format!("{PROFILE_API}/skins"))
        .bearer_auth(&session.token)
        .header(
            reqwest::header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(multipart(&boundary, variant, &png))
        .send()
        .await
        .map_err(err)?;
    updated_profile(&state, &session.token, response, "skin").await
}

#[tauri::command]
pub async fn set_cape(
    id: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<auth::Profile> {
    let session = signed_in(&state).await?;
    let request = match id.filter(|id| !id.trim().is_empty()) {
        Some(id) => {
            if !session.profile.capes.iter().any(|cape| cape.id == id) {
                return Err("Cette cape n’appartient pas à ce compte.".into());
            }
            http()
                .put(format!("{PROFILE_API}/capes/active"))
                .json(&serde_json::json!({ "capeId": id }))
        }
        None => http().delete(format!("{PROFILE_API}/capes/active")),
    };
    let response = request
        .bearer_auth(&session.token)
        .send()
        .await
        .map_err(err)?;
    updated_profile(&state, &session.token, response, "cape").await
}

#[tauri::command]
pub fn read_skin_file(path: String) -> Result<String> {
    let path = PathBuf::from(path);
    let size = std::fs::metadata(&path).map_err(err)?.len();
    if size > MAX_SKIN_BYTES as u64 {
        return Err("Skin trop volumineux (256 Ko maximum).".into());
    }
    let bytes = std::fs::read(&path).map_err(err)?;
    validate_skin(&bytes)?;
    Ok(data_url(&bytes))
}

#[tauri::command]
pub fn export_skin(path: String, data: String) -> Result<()> {
    let png = decode_skin(&data)?;
    let mut path = PathBuf::from(path);
    if path.extension().and_then(|value| value.to_str()) != Some("png") {
        path.set_extension("png");
    }
    std::fs::write(path, png).map_err(err)
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LibraryEntry {
    id: String,
    name: String,
    variant: String,
    updated_at: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedSkin {
    id: String,
    name: String,
    variant: String,
    data: String,
}

fn library_dir(state: &AppState) -> PathBuf {
    state.root.join("skins")
}

fn read_index(dir: &Path) -> Result<Vec<LibraryEntry>> {
    match std::fs::read(dir.join("index.json")) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(err),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(err(e)),
    }
}

fn write_index(dir: &Path, entries: &[LibraryEntry]) -> Result<()> {
    let tmp = dir.join("index.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(entries).map_err(err)?).map_err(err)?;
    std::fs::rename(tmp, dir.join("index.json")).map_err(err)
}

fn safe_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

fn list(dir: &Path) -> Result<Vec<SavedSkin>> {
    Ok(read_index(dir)?
        .into_iter()
        .filter(|entry| safe_id(&entry.id))
        .filter_map(|entry| {
            let bytes = std::fs::read(dir.join(format!("{}.png", entry.id))).ok()?;
            validate_skin(&bytes).ok()?;
            Some(SavedSkin {
                data: data_url(&bytes),
                id: entry.id,
                name: entry.name,
                variant: entry.variant,
            })
        })
        .collect())
}

fn save(
    dir: &Path,
    id: Option<String>,
    name: &str,
    variant: &str,
    data: &str,
) -> Result<SavedSkin> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 40 {
        return Err("Choisis un nom de 1 à 40 caractères.".into());
    }
    let variant = self::variant(variant)?;
    let png = decode_skin(data)?;
    std::fs::create_dir_all(dir).map_err(err)?;
    let mut entries = read_index(dir)?;
    let existing = id.and_then(|id| entries.iter().position(|entry| entry.id == id));
    let id = match existing {
        Some(index) => entries.remove(index).id,
        None => {
            let base = format!("skin-{}", now_millis());
            let mut id = base.clone();
            let mut suffix = 1;
            while entries.iter().any(|entry| entry.id == id) {
                suffix += 1;
                id = format!("{base}-{suffix}");
            }
            id
        }
    };
    if !safe_id(&id) {
        return Err("Identifiant de skin invalide.".into());
    }
    std::fs::write(dir.join(format!("{id}.png")), &png).map_err(err)?;
    entries.insert(
        0,
        LibraryEntry {
            id: id.clone(),
            name: name.into(),
            variant: variant.into(),
            updated_at: (now_millis() / 1000) as u64,
        },
    );
    write_index(dir, &entries)?;
    Ok(SavedSkin {
        id,
        name: name.into(),
        variant: variant.into(),
        data: data_url(&png),
    })
}

fn delete(dir: &Path, id: &str) -> Result<()> {
    let mut entries = read_index(dir)?;
    let index = entries
        .iter()
        .position(|entry| entry.id == id)
        .ok_or("Skin introuvable.")?;
    let entry = entries.remove(index);
    if safe_id(&entry.id) {
        match std::fs::remove_file(dir.join(format!("{}.png", entry.id))) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(err(e)),
        }
    }
    write_index(dir, &entries)
}

#[tauri::command]
pub fn list_skins(state: tauri::State<AppState>) -> Result<Vec<SavedSkin>> {
    list(&library_dir(&state))
}

#[tauri::command]
pub fn save_skin(
    id: Option<String>,
    name: String,
    variant: String,
    data: String,
    state: tauri::State<AppState>,
) -> Result<SavedSkin> {
    save(&library_dir(&state), id, &name, &variant, &data)
}

#[tauri::command]
pub fn delete_skin(id: String, state: tauri::State<AppState>) -> Result<()> {
    delete(&library_dir(&state), &id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = PNG_SIGNATURE.to_vec();
        bytes.extend_from_slice(&13u32.to_be_bytes());
        bytes.extend_from_slice(b"IHDR");
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes.extend_from_slice(&[8, 6, 0, 0, 0]);
        bytes
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "hxlauncher-skins-{name}-{}-{}",
            std::process::id(),
            now_millis()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn accepts_only_minecraft_skin_sizes() {
        assert!(validate_skin(&png(64, 64)).is_ok());
        assert!(validate_skin(&png(64, 32)).is_ok());
        assert!(validate_skin(&png(128, 128)).is_err());
        assert!(validate_skin(&png(32, 64)).is_err());
        assert!(validate_skin(b"GIF89a not a png at all....").is_err());
        let mut huge = png(64, 64);
        huge.resize(MAX_SKIN_BYTES + 1, 0);
        assert!(validate_skin(&huge).is_err());
    }

    #[test]
    fn decodes_data_urls_and_raw_base64() {
        let url = data_url(&png(64, 64));
        assert_eq!(decode_skin(&url).unwrap(), png(64, 64));
        let raw = url.strip_prefix(DATA_URL_PREFIX).unwrap();
        assert!(decode_skin(raw).is_ok());
        assert!(decode_skin("data:image/png;base64,***").is_err());
    }

    #[test]
    fn maps_editor_models_to_mojang_variants() {
        assert_eq!(variant("default").unwrap(), "classic");
        assert_eq!(variant("CLASSIC").unwrap(), "classic");
        assert_eq!(variant("slim").unwrap(), "slim");
        assert!(variant("wide").is_err());
    }

    #[test]
    fn builds_multipart_skin_upload() {
        let body = multipart("b", "slim", b"PNG");
        let text = String::from_utf8(body).unwrap();
        assert!(text.starts_with(
            "--b\r\nContent-Disposition: form-data; name=\"variant\"\r\n\r\nslim\r\n"
        ));
        assert!(text.contains("name=\"file\"; filename=\"skin.png\"\r\nContent-Type: image/png\r\n\r\nPNG\r\n--b--\r\n"));
    }

    #[test]
    fn library_saves_updates_lists_and_deletes() {
        let dir = temp_dir("library");
        let skin = data_url(&png(64, 64));
        let first = save(&dir, None, "  Chevalier ", "default", &skin).unwrap();
        assert_eq!(first.name, "Chevalier");
        assert_eq!(first.variant, "classic");
        let second = save(&dir, None, "Alex", "slim", &skin).unwrap();
        assert_ne!(first.id, second.id);
        let names: Vec<_> = list(&dir).unwrap().into_iter().map(|s| s.name).collect();
        assert_eq!(names, ["Alex", "Chevalier"]);

        let renamed = save(&dir, Some(first.id.clone()), "Paladin", "slim", &skin).unwrap();
        assert_eq!(renamed.id, first.id);
        let saved = list(&dir).unwrap();
        assert_eq!(saved.len(), 2);
        assert_eq!(saved[0].name, "Paladin");
        assert_eq!(saved[0].variant, "slim");

        delete(&dir, &first.id).unwrap();
        assert!(!dir.join(format!("{}.png", first.id)).exists());
        assert_eq!(list(&dir).unwrap().len(), 1);
        assert!(delete(&dir, "../state").is_err());
        assert!(save(&dir, None, "", "slim", &skin).is_err());
        assert!(save(&dir, None, "Géant", "slim", &data_url(&png(128, 128))).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
