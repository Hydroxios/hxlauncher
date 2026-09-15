use crate::{
    auth, err,
    minecraft::{download, progress},
    AppState, Instance, Result, Settings,
};
use mc_launcher_core::{install::client, loader, prelude::*, progress::ProgressEvent};
use std::{path::PathBuf, process::Stdio, time::Duration};

pub fn identifier(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 160
        || !value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b".-_".contains(&c))
        || value == "."
        || value == ".."
    {
        return Err("Identifiant de version invalide.".into());
    }
    Ok(())
}
fn reporter(app: tauri::AppHandle) -> impl FnMut(ProgressEvent) {
    let mut last = std::time::Instant::now();
    move |event| {
        if last.elapsed() < Duration::from_millis(150) {
            return;
        }
        if let ProgressEvent::BytesReceived {
            received, total, ..
        } = event
        {
            progress(
                &app,
                "download",
                "Installation de Minecraft et des bibliothèques…",
                received as usize,
                total.unwrap_or(0) as usize,
            );
            last = std::time::Instant::now();
        }
    }
}
pub async fn java_check(java: &str, required: i32) -> Result<()> {
    let out = tokio::time::timeout(
        Duration::from_secs(10),
        tokio::process::Command::new(java)
            .arg("-version")
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| "Java ne répond pas.")?
    .map_err(|_| "Java introuvable. Indique son chemin dans les réglages.")?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
    let re = regex::Regex::new(r#"version "(?:1\.)?(\d+)"#).map_err(err)?;
    let major = re
        .captures(&text)
        .and_then(|c| c[1].parse::<i32>().ok())
        .unwrap_or(0);
    if !out.status.success() || major < required {
        return Err(format!(
            "Ce pack nécessite Java {required} ou supérieur. Vérifie Java dans les réglages."
        ));
    }
    Ok(())
}
pub async fn install(
    root: PathBuf,
    mc: String,
    spec: String,
    java: String,
    app: tauri::AppHandle,
) -> Result<String> {
    identifier(&mc)?;
    let (kind, version) = if spec == "Vanilla" {
        ("vanilla", "")
    } else {
        spec.split_once('-').ok_or("Modloader inconnu.")?
    };
    if !matches!(kind, "vanilla" | "fabric" | "quilt" | "forge" | "neoforge") {
        return Err(format!("Modloader non pris en charge : {kind}"));
    }
    if kind != "vanilla" {
        identifier(version)?;
    }
    progress(&app, "prepare", "Préparation de Minecraft…", 0, 0);
    let mc2 = mc.clone();
    let base = tauri::async_runtime::spawn_blocking(move || {
        client::fetch_vanilla_version(&mc2).map_err(err)
    })
    .await
    .map_err(err)??;
    java_check(
        &java,
        base.java_version
            .as_ref()
            .map(|j| j.major_version)
            .unwrap_or(8),
    )
    .await?;
    let r = root.clone();
    tauri::async_runtime::spawn_blocking(move || client::write_version_json(r, &base).map_err(err))
        .await
        .map_err(err)??;
    let id = match kind {
        "vanilla" => mc.clone(),
        "fabric" | "quilt" => {
            let (m, v, k, r) = (
                mc.clone(),
                version.to_owned(),
                kind.to_owned(),
                root.clone(),
            );
            tauri::async_runtime::spawn_blocking(move || -> Result<String> {
                let profile = if k == "fabric" {
                    loader::fabric::fetch_profile(&m, &v)
                } else {
                    loader::quilt::fetch_profile(&m, &v)
                }
                .map_err(err)?;
                let id = profile.id.clone().ok_or("Profil du modloader absent.")?;
                identifier(&id)?;
                if profile.inherits_from.as_deref() != Some(&m) {
                    return Err("Version du modloader incompatible.".into());
                }
                client::write_version_json(r, &profile).map_err(err)?;
                Ok(id)
            })
            .await
            .map_err(err)??
        }
        _ => {
            // Forge installers need a real base client before running their processors.
            install_files(root.clone(), mc.clone(), app.clone()).await?;
            let (url, id) = if kind == "forge" {
                let full = if version.starts_with(&format!("{mc}-")) {
                    version.to_owned()
                } else {
                    format!("{mc}-{version}")
                };
                (
                    loader::forge::installer_url(&full),
                    loader::forge::forge_installed_version_id(&full).map_err(err)?,
                )
            } else {
                (
                    loader::neoforge::installer_url(version),
                    loader::neoforge::neoforge_installed_version_id(&mc, version),
                )
            };
            identifier(&id)?;
            let jar = root.join(format!("{id}-installer.jar"));
            progress(&app, "prepare", format!("Installation de {spec}…"), 0, 0);
            download(&url, &jar, None, 128 * 1024 * 1024).await?;
            let profiles = root.join("launcher_profiles.json");
            if !profiles.exists() {
                tokio::fs::write(profiles, b"{\"profiles\":{}}")
                    .await
                    .map_err(err)?;
            }
            let log_path = root.join("loader-install.log");
            let log = std::fs::File::create(&log_path).map_err(err)?;
            let status = tokio::time::timeout(
                Duration::from_secs(900),
                tokio::process::Command::new(&java)
                    .arg("-jar")
                    .arg(&jar)
                    .arg("--installClient")
                    .arg(&root)
                    .current_dir(&root)
                    .stdout(Stdio::from(log.try_clone().map_err(err)?))
                    .stderr(Stdio::from(log))
                    .kill_on_drop(true)
                    .status(),
            )
            .await
            .map_err(|_| "Le modloader n’a pas terminé après 15 minutes.")?
            .map_err(err)?;
            if !status.success() {
                return Err(format!(
                    "Échec du modloader. Journal : {}",
                    log_path.display()
                ));
            }
            id
        }
    };
    install_files(root, id.clone(), app).await?;
    Ok(id)
}
async fn install_files(root: PathBuf, id: String, app: tauri::AppHandle) -> Result<()> {
    tauri::async_runtime::spawn_blocking(move || {
        let version = client::load_version_json(&root, &id).map_err(err)?;
        client::install_version_files(&version, &root, &mut reporter(app)).map_err(err)
    })
    .await
    .map_err(err)?
}
pub async fn launch(
    instance: Instance,
    settings: Settings,
    app: &tauri::AppHandle,
    state: &AppState,
) -> Result<()> {
    progress(app, "prepare", "Vérification du compte…", 0, 0);
    let session = auth::refresh(state)
        .await?
        .ok_or("Connecte-toi avec Microsoft avant de jouer.")?;
    let game = crate::minecraft::safe_path(&state.root.join("instances"), &instance.id)?;
    let root = state.root.join("modded-runtime");
    let id = instance.profile_id.ok_or("Profil absent.")?;
    identifier(&id)?;
    let java = settings.java_path.clone();
    let command = tauri::async_runtime::spawn_blocking(move || -> Result<_> {
        let launcher = Launcher::new(root);
        let version = launcher.load_version(&id).map_err(err)?;
        let required = version
            .java_version
            .as_ref()
            .map(|j| j.major_version)
            .unwrap_or(8);
        let command = launcher
            .build_launch_command_from_version(
                &version,
                LaunchOptions {
                    account: Account::Microsoft {
                        username: session.profile.name,
                        uuid: session.profile.id,
                        access_token: session.token,
                    },
                    java_executable: Some(PathBuf::from(java)),
                    game_directory: Some(game),
                    launcher_name: "HX Launcher".into(),
                    ..Default::default()
                },
            )
            .map_err(err)?;
        Ok((command, required))
    })
    .await
    .map_err(err)??;
    java_check(&settings.java_path, command.1).await?;
    let cmd = command.0;
    let log_path = cmd.working_dir.join("launcher-game.log");
    let log = std::fs::File::create(&log_path).map_err(err)?;
    let mut child = tokio::process::Command::new(cmd.executable)
        .arg("-Xms512M")
        .arg(format!("-Xmx{}M", settings.memory_mb))
        .args(cmd.args)
        .envs(cmd.env)
        .current_dir(cmd.working_dir)
        .stdout(Stdio::from(log.try_clone().map_err(err)?))
        .stderr(Stdio::from(log))
        .spawn()
        .map_err(err)?;
    progress(app, "running", "Minecraft est lancé.", 0, 0);
    if !child.wait().await.map_err(err)?.success() {
        return Err(format!(
            "Minecraft s’est arrêté avec une erreur. Journal : {}",
            log_path.display()
        ));
    }
    progress(app, "idle", "Minecraft est fermé.", 0, 0);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_untrusted_version_paths() {
        for id in ["../evil", "a/b", "", "..", "a?b", "a#b", "-jar /tmp/evil"] {
            assert!(identifier(id).is_err());
        }
        for id in ["1.20.1", "47.3.0", "fabric-loader-0.16.10-1.21.1"] {
            assert!(identifier(id).is_ok());
        }
    }
    #[test]
    #[ignore = "requires access to official Mojang and Fabric metadata"]
    fn official_fabric_profile_is_launchable() {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
        let base = client::fetch_vanilla_version("1.20.1").unwrap();
        let fabric = loader::fabric::fetch_profile("1.20.1", "0.16.10").unwrap();
        assert_eq!(fabric.inherits_from.as_deref(), Some("1.20.1"));
        let merged = base.merge_child(&fabric);
        let launcher = Launcher::new(std::env::temp_dir().join("hx-command-smoke"));
        let command = launcher
            .build_launch_command_from_version(
                &merged,
                LaunchOptions {
                    account: Account::Microsoft {
                        username: "FixturePlayer".into(),
                        uuid: "00000000000000000000000000000001".into(),
                        access_token: "fixture-token".into(),
                    },
                    game_directory: Some(std::env::temp_dir().join("hx-fixture-game")),
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(command
            .args
            .iter()
            .any(|a| a == "net.fabricmc.loader.impl.launch.knot.KnotClient"));
        assert!(command.args.iter().any(|a| a == "FixturePlayer"));
        assert!(!command.args.iter().any(|a| a.contains("${")));
    }
}
