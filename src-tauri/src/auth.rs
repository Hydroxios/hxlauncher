//! OAuth is delegated to oauth2-rs; Xbox/Minecraft exchanges to minecraft-msa-auth.
//! Only profile and human-readable device codes cross the Tauri boundary.
use crate::{err, http, AppState, Result};
use minecraft_msa_auth::MinecraftAuthorizationFlow;
use oauth2::{
    basic::BasicClient, ClientId, DeviceAuthorizationUrl, EndpointNotSet, EndpointSet,
    RefreshToken, Scope, StandardDeviceAuthorizationResponse, TokenResponse, TokenUrl,
};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

// Keep only a sanitized diagnosis; never retain response bodies or tokens.
struct DiagnosticAuthClient {
    client: auth_http::Client,
    failure: std::sync::Mutex<Option<String>>,
}

fn auth_failure(host: &str, status: u16, body: &[u8]) -> String {
    let stage = match host {
        "user.auth.xboxlive.com" => "Xbox Live",
        "xsts.auth.xboxlive.com" => "Xbox XSTS",
        "api.minecraftservices.com" => "Minecraft Services",
        _ => "Service d’authentification",
    };
    let invalid_app = serde_json::from_slice::<serde_json::Value>(body)
        .ok()
        .and_then(|value| {
            value
                .get("errorMessage")
                .and_then(|v| v.as_str())
                .map(str::to_owned)
        })
        .is_some_and(|message| message.eq_ignore_ascii_case("Invalid app registration"));
    if host == "api.minecraftservices.com" && status == 403 && invalid_app {
        return "Minecraft Services refuse l’inscription de cette application (HTTP 403 : Invalid app registration). Demande la validation de ton Client ID auprès de Mojang : https://aka.ms/mce-reviewappid".into();
    }
    let hint = if host == "api.minecraftservices.com" && status == 403 {
        " L’autorisation de cette application auprès de Mojang est à vérifier ; ce statut seul ne confirme pas la cause."
    } else {
        ""
    };
    format!("{stage} : HTTP {status}.{hint}")
}

#[async_trait::async_trait]
impl minecraft_msa_auth::HttpClient for DiagnosticAuthClient {
    type Error = auth_http::Error;

    async fn call(
        &self,
        request: minecraft_msa_auth::HttpRequest,
    ) -> std::result::Result<minecraft_msa_auth::HttpResponse, Self::Error> {
        let host = request.uri().host().unwrap_or_default().to_owned();
        let response = minecraft_msa_auth::HttpClient::call(&self.client, request).await?;
        if !response.status().is_success() {
            if let Ok(mut failure) = self.failure.lock() {
                *failure = Some(auth_failure(
                    &host,
                    response.status().as_u16(),
                    response.body(),
                ));
            }
        }
        Ok(response)
    }
}

#[async_trait::async_trait]
impl minecraft_msa_auth::HttpClient for &DiagnosticAuthClient {
    type Error = auth_http::Error;

    async fn call(
        &self,
        request: minecraft_msa_auth::HttpRequest,
    ) -> std::result::Result<minecraft_msa_auth::HttpResponse, Self::Error> {
        minecraft_msa_auth::HttpClient::call(*self, request).await
    }
}
const BASE: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0";
type OAuth = BasicClient<EndpointNotSet, EndpointSet, EndpointNotSet, EndpointNotSet, EndpointSet>;
fn oauth(client_id: &str) -> Result<OAuth> {
    Ok(BasicClient::new(ClientId::new(client_id.into()))
        .set_token_uri(TokenUrl::new(format!("{BASE}/token")).map_err(err)?)
        .set_device_authorization_url(
            DeviceAuthorizationUrl::new(format!("{BASE}/devicecode")).map_err(err)?,
        ))
}
fn oauth_http() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(err)
}
#[derive(Clone, Deserialize, Serialize)]
pub struct Skin {
    pub url: String,
    pub variant: String,
    pub state: String,
}
#[derive(Clone, Deserialize, Serialize)]
pub struct Cape {
    pub url: String,
    pub state: String,
}
#[derive(Clone, Deserialize, Serialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub skins: Vec<Skin>,
    #[serde(default)]
    pub capes: Vec<Cape>,
}
#[derive(Clone, Deserialize, Serialize)]
pub struct Session {
    pub profile: Profile,
    pub token: String,
    expires_at: u64,
}
impl Session {
    fn is_valid_at(&self, now: u64) -> bool {
        !self.token.is_empty() && self.expires_at > now.saturating_add(120)
    }
}
#[derive(Deserialize, Serialize)]
struct Credentials {
    refresh_token: String,
    session: Option<Session>,
}
impl Credentials {
    fn decode(value: String) -> Self {
        // Earlier versions stored only the Microsoft refresh token.
        serde_json::from_str(&value).unwrap_or(Self {
            refresh_token: value,
            session: None,
        })
    }
    fn save(&self, client: &str) -> Result<()> {
        entry(client)?
            .set_password(&serde_json::to_string(self).map_err(err)?)
            .map_err(|e| format!("Impossible de sécuriser la session dans le trousseau : {e}"))
    }
}
fn now_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
#[derive(Clone)]
pub struct Pending {
    details: StandardDeviceAuthorizationResponse,
    client_id: String,
    cancel: CancellationToken,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceCode {
    user_code: String,
    verification_uri: String,
    expires_in: u64,
    interval: u64,
}
fn entry(client: &str) -> Result<keyring::Entry> {
    keyring::Entry::new("dev.hydro.hxlauncher", client).map_err(err)
}
async fn minecraft_session(access: &str) -> Result<Session> {
    let started_at = now_seconds();
    let client = auth_http::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .redirect(auth_http::redirect::Policy::none())
        .build()
        .map_err(err)?;
    let client = DiagnosticAuthClient {
        client,
        failure: std::sync::Mutex::new(None),
    };
    let response = MinecraftAuthorizationFlow::new(&client)
        .exchange_microsoft_token(access)
        .await
        .map_err(|e| match e {
            minecraft_msa_auth::MinecraftAuthorizationError::HttpStatus(_) => client
                .failure
                .lock()
                .ok()
                .and_then(|failure| failure.clone())
                .unwrap_or_else(|| "Connexion Xbox/Minecraft refusée.".into()),
            minecraft_msa_auth::MinecraftAuthorizationError::NoXbox => {
                "Crée ton profil Xbox sur xbox.com, puis reconnecte-toi.".into()
            }
            minecraft_msa_auth::MinecraftAuthorizationError::AddToFamily => {
                "Xbox exige que ce compte mineur soit ajouté à une famille Microsoft.".into()
            }
            _ => format!("Connexion Xbox/Minecraft : {e}"),
        })?;
    let token: String = response.access_token().clone().into();
    let profile = http()
        .get("https://api.minecraftservices.com/minecraft/profile")
        .timeout(std::time::Duration::from_secs(30))
        .bearer_auth(&token)
        .send()
        .await
        .map_err(err)?;
    if !profile.status().is_success() {
        return Err("Aucun profil Minecraft Java accessible. Vérifie ta licence et la création du profil sur minecraft.net.".into());
    }
    Ok(Session {
        profile: profile.json().await.map_err(err)?,
        token,
        expires_at: started_at.saturating_add(response.expires_in() as u64),
    })
}
#[tauri::command]
pub async fn begin_login(state: tauri::State<'_, AppState>) -> Result<DeviceCode> {
    let client_id = crate::microsoft_client_id();
    if client_id.is_empty() {
        return Err("MICROSOFT_CLIENT_ID absent de la configuration du launcher.".into());
    }
    let details: StandardDeviceAuthorizationResponse = oauth(&client_id)?.exchange_device_code().add_scope(Scope::new("XboxLive.signin".into())).add_scope(Scope::new("offline_access".into())).request_async(&oauth_http()?).await.map_err(|_| "Microsoft refuse cette application. Vérifie le Client ID et l’activation des flux clients publics.")?;
    let result = DeviceCode {
        user_code: details.user_code().secret().clone(),
        verification_uri: details.verification_uri().to_string(),
        expires_in: details.expires_in().as_secs(),
        interval: details.interval().as_secs(),
    };
    let mut pending = state.pending.lock().map_err(err)?;
    if let Some(old) = pending.take() {
        old.cancel.cancel();
    }
    *pending = Some(Pending {
        details,
        client_id,
        cancel: CancellationToken::new(),
    });
    Ok(result)
}
#[tauri::command]
pub async fn poll_login(state: tauri::State<'_, AppState>) -> Result<Option<Profile>> {
    let p = state
        .pending
        .lock()
        .map_err(err)?
        .clone()
        .ok_or("Connexion annulée.")?;
    let client = oauth(&p.client_id)?;
    let http = oauth_http()?;
    let token = tokio::select! {
        _ = p.cancel.cancelled() => return Err("Connexion annulée.".into()),
        result = client.exchange_device_access_token(&p.details).request_async(&http, tokio::time::sleep, None) => result.map_err(|_| "Connexion Microsoft refusée ou expirée. Relance la connexion.")?
    };
    let session = tokio::select! { _ = p.cancel.cancelled() => return Err("Connexion annulée.".into()), result = minecraft_session(token.access_token().secret()) => result? };
    let mut guard = state.pending.lock().map_err(err)?;
    if p.cancel.is_cancelled() || guard.is_none() {
        return Err("Connexion annulée.".into());
    }
    let refresh = token
        .refresh_token()
        .ok_or("Microsoft n’a pas fourni de jeton de renouvellement.")?;
    Credentials {
        refresh_token: refresh.secret().clone(),
        session: Some(session.clone()),
    }
    .save(&p.client_id)?;
    *guard = None;
    let profile = session.profile.clone();
    *state.session.lock().map_err(err)? = Some(session);
    Ok(Some(profile))
}
pub async fn refresh(state: &AppState) -> Result<Option<Session>> {
    if let Some(session) = state.session.lock().map_err(err)?.as_ref() {
        if session.is_valid_at(now_seconds()) {
            return Ok(Some(session.clone()));
        }
    }
    let client = crate::microsoft_client_id();
    if client.is_empty() {
        return Ok(None);
    }
    let refresh = match entry(&client)?.get_password() {
        Ok(t) => t,
        Err(keyring::Error::NoEntry) => return Ok(None),
        Err(e) => return Err(format!("Trousseau inaccessible : {e}")),
    };
    let mut credentials = Credentials::decode(refresh);
    if let Some(session) = &credentials.session {
        if session.is_valid_at(now_seconds()) {
            return Ok(Some(session.clone()));
        }
    }
    let token = oauth(&client)?
        .exchange_refresh_token(&RefreshToken::new(credentials.refresh_token.clone()))
        .request_async(&oauth_http()?)
        .await
        .map_err(|_| "Session Microsoft expirée ou service indisponible. Reconnecte-toi.")?;
    if let Some(refresh) = token.refresh_token() {
        credentials.refresh_token = refresh.secret().clone();
        credentials.session = None;
        credentials.save(&client)?;
    }
    let session = minecraft_session(token.access_token().secret()).await?;
    credentials.session = Some(session.clone());
    credentials.save(&client)?;
    Ok(Some(session))
}
#[tauri::command]
pub async fn restore_session(state: tauri::State<'_, AppState>) -> Result<Option<Profile>> {
    let _guard = state
        .busy
        .try_lock()
        .map_err(|_| "Une opération est déjà en cours.")?;
    let session = refresh(&state).await?;
    let profile = session.as_ref().map(|s| s.profile.clone());
    *state.session.lock().map_err(err)? = session;
    Ok(profile)
}
#[tauri::command]
pub fn cancel_login(state: tauri::State<AppState>) -> Result<()> {
    if let Some(p) = state.pending.lock().map_err(err)?.take() {
        p.cancel.cancel();
    }
    Ok(())
}
#[tauri::command]
pub async fn logout(state: tauri::State<'_, AppState>) -> Result<()> {
    let _guard = state
        .busy
        .try_lock()
        .map_err(|_| "Attends la fin de l’opération avant de te déconnecter.")?;
    if let Some(p) = state.pending.lock().map_err(err)?.take() {
        p.cancel.cancel();
    }
    let client = crate::microsoft_client_id();
    if !client.is_empty() {
        match entry(&client)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => {}
            Err(e) => return Err(err(e)),
        }
    }
    *state.session.lock().map_err(err)? = None;
    Ok(())
}

/// Restrict the image proxy to Mojang textures; no bearer token is sent.
#[tauri::command]
pub async fn skin_texture(url: String) -> Result<String> {
    use base64::Engine;
    use futures_util::StreamExt;
    let url = texture_url(&url)?;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(err)?;
    let response = client
        .get(url)
        .send()
        .await
        .map_err(err)?
        .error_for_status()
        .map_err(err)?;
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(err)?;
        if bytes.len() + chunk.len() > 256 * 1024 {
            return Err("Texture trop volumineuse.".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err("Texture PNG invalide.".into());
    }
    Ok(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

fn texture_url(source: &str) -> Result<reqwest::Url> {
    let mut url = reqwest::Url::parse(source.trim()).map_err(err)?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str() != Some("textures.minecraft.net")
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || !url.path().starts_with("/texture/")
    {
        return Err("URL de skin Mojang invalide.".into());
    }
    // Minecraft profiles can contain legacy HTTP texture URLs. Never fetch them
    // over HTTP: validate the Mojang host first, then upgrade the transport.
    url.set_scheme("https")
        .map_err(|_| "URL de skin invalide.")?;
    Ok(url)
}

#[cfg(test)]
mod skin_tests {
    use super::*;
    #[test]
    fn cached_session_requires_unexpired_token_with_refresh_margin() {
        let mut session = Session {
            profile: Profile {
                id: "id".into(),
                name: "Alex".into(),
                skins: vec![],
                capes: vec![],
            },
            token: "test-token".into(),
            expires_at: 1_000,
        };
        assert!(session.is_valid_at(879));
        assert!(!session.is_valid_at(880));
        assert!(!session.is_valid_at(1_001));
        session.token.clear();
        assert!(!session.is_valid_at(0));
    }
    #[test]
    fn credentials_support_legacy_tokens_and_cached_sessions() {
        let legacy = Credentials::decode("legacy-refresh-token".into());
        assert_eq!(legacy.refresh_token, "legacy-refresh-token");
        assert!(legacy.session.is_none());
        let cached = Credentials::decode(r#"{"refresh_token":"refresh","session":{"profile":{"id":"id","name":"Alex"},"token":"test-token","expires_at":1000}}"#.into());
        assert_eq!(cached.refresh_token, "refresh");
        let roundtrip = Credentials::decode(serde_json::to_string(&cached).unwrap());
        let session = roundtrip.session.unwrap();
        assert_eq!(session.profile.name, "Alex");
        assert!(session.is_valid_at(100));
        assert!(!session.is_valid_at(1000));
    }
    #[test]
    fn auth_diagnosis_distinguishes_service_and_confirmed_registration_error() {
        let body = br#"{"errorMessage":"Invalid app registration","access_token":"SECRET"}"#;
        let confirmed = auth_failure("api.minecraftservices.com", 403, body);
        assert!(confirmed.contains("Demande la validation"));
        assert!(!confirmed.contains("SECRET"));
        let unknown = auth_failure("api.minecraftservices.com", 403, b"Forbidden");
        assert!(unknown.contains("ne confirme pas"));
        let xbox = auth_failure("xsts.auth.xboxlive.com", 403, body);
        assert!(xbox.starts_with("Xbox XSTS"));
        assert!(!xbox.contains("Mojang"));
    }
    #[test]
    fn preserves_active_skin_model() {
        let profile: Profile = serde_json::from_str(r#"{"id":"player","name":"Alex","skins":[{"url":"https://textures.minecraft.net/texture/abc","variant":"SLIM","state":"ACTIVE"}]}"#).unwrap();
        assert_eq!(profile.skins[0].variant, "SLIM");
        assert_eq!(profile.skins[0].state, "ACTIVE");
        let legacy: Profile = serde_json::from_str(r#"{"id":"player","name":"Steve"}"#).unwrap();
        assert!(legacy.skins.is_empty());
        assert!(legacy.capes.is_empty());
        let with_capes: Profile = serde_json::from_str(r#"{"id":"player","name":"Alex","capes":[{"url":"http://textures.minecraft.net/texture/cape","state":"ACTIVE","alias":"Migrator"}]}"#).unwrap();
        assert_eq!(with_capes.capes[0].state, "ACTIVE");
        let serialized = serde_json::to_value(&with_capes).unwrap();
        assert_eq!(
            serialized["capes"][0]["url"],
            "http://textures.minecraft.net/texture/cape"
        );
    }
    #[test]
    fn upgrades_legacy_mojang_textures_to_https() {
        assert_eq!(
            texture_url("http://textures.minecraft.net/texture/abc")
                .unwrap()
                .as_str(),
            "https://textures.minecraft.net/texture/abc"
        );
        assert!(texture_url("https://textures.minecraft.net:444/texture/abc").is_err());
        assert!(texture_url("https://user@textures.minecraft.net/texture/abc").is_err());
    }
    #[tokio::test]
    #[ignore = "downloads a public Mojang skin texture"]
    async fn official_http_texture_loads_as_png() {
        use base64::Engine;
        let image = skin_texture("http://textures.minecraft.net/texture/dae0995f0429b89b7c23b8a561195c14aaefad01ab42a5a4953dd815c7e58eb7".into()).await.unwrap();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(image.strip_prefix("data:image/png;base64,").unwrap())
            .unwrap();
        assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
    }
    #[tokio::test]
    async fn refuses_untrusted_texture_sources() {
        for url in [
            "http://example.com/texture/abc",
            "https://example.com/texture/abc",
            "https://textures.minecraft.net.evil.test/texture/abc",
            "https://textures.minecraft.net/other",
        ] {
            assert!(skin_texture(url.into()).await.is_err());
        }
    }
}
