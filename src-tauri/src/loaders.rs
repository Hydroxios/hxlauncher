//! Official loader catalogues, filtered to the selected Minecraft release.
use crate::{err, http, modded::identifier, Result};
use mc_launcher_core::loader;
use serde::Deserialize;
use std::cmp::Ordering;

#[derive(Deserialize)]
struct MetaEntry {
    loader: MetaLoader,
}

#[derive(Deserialize)]
struct MetaLoader {
    version: String,
    stable: Option<bool>,
}

// Compare numeric components so 52.0.10 sorts ahead of 52.0.9.
fn version_order(a: &str, b: &str) -> Ordering {
    let parts = |v: &str| {
        v.split(['.', '-', '_'])
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    let a = parts(a);
    let b = parts(b);
    for (a, b) in a.iter().zip(&b) {
        let order = match (a.parse::<u64>(), b.parse::<u64>()) {
            (Ok(a), Ok(b)) => a.cmp(&b),
            _ => a.cmp(b),
        };
        if order != Ordering::Equal {
            return order;
        }
    }
    a.len().cmp(&b.len())
}

fn sorted_versions(mut entries: Vec<MetaLoader>) -> Vec<String> {
    entries.retain(|entry| identifier(&entry.version).is_ok());
    let stable = |entry: &MetaLoader| {
        entry
            .stable
            .unwrap_or_else(|| !entry.version.bytes().any(|c| c.is_ascii_alphabetic()))
    };
    entries.sort_by(|a, b| {
        stable(b)
            .cmp(&stable(a))
            .then_with(|| version_order(&b.version, &a.version))
    });
    entries.dedup_by(|a, b| a.version == b.version);
    entries.into_iter().map(|entry| entry.version).collect()
}

fn maven_versions(kind: &str, minecraft: &str, versions: Vec<String>) -> Vec<MetaLoader> {
    versions
        .into_iter()
        .filter_map(|version| {
            let version = if kind == "forge" {
                version.strip_prefix(&format!("{minecraft}-"))?.to_owned()
            } else {
                loader::neoforge::latest_for_minecraft(std::slice::from_ref(&version), minecraft)
                    .ok()?;
                version
            };
            Some(MetaLoader {
                version,
                stable: None,
            })
        })
        .collect()
}

#[tauri::command]
pub async fn list_loader_versions(minecraft: String, loader: String) -> Result<Vec<String>> {
    identifier(&minecraft)?;
    let url = match loader.as_str() {
        "Vanilla" => return Ok(Vec::new()),
        "fabric" => format!("https://meta.fabricmc.net/v2/versions/loader/{minecraft}"),
        "quilt" => format!("https://meta.quiltmc.org/v3/versions/loader/{minecraft}"),
        "forge" => {
            "https://maven.minecraftforge.net/net/minecraftforge/forge/maven-metadata.xml".into()
        }
        "neoforge" => {
            "https://maven.neoforged.net/releases/net/neoforged/neoforge/maven-metadata.xml".into()
        }
        _ => return Err("Modloader non pris en charge.".into()),
    };
    let response = http()
        .get(url)
        .send()
        .await
        .map_err(err)?
        .error_for_status()
        .map_err(err)?;
    let entries = if matches!(loader.as_str(), "fabric" | "quilt") {
        response
            .json::<Vec<MetaEntry>>()
            .await
            .map_err(err)?
            .into_iter()
            .map(|entry| entry.loader)
            .collect()
    } else {
        let xml = response.text().await.map_err(err)?;
        let metadata = loader::forge::parse_maven_metadata(&xml).map_err(err)?;
        maven_versions(&loader, &minecraft, metadata.versions)
    };
    Ok(sorted_versions(entries))
}

pub async fn validate_spec(minecraft: &str, spec: &str) -> Result<()> {
    if spec == "Vanilla" {
        return Ok(());
    }
    let (kind, version) = spec
        .split_once('-')
        .ok_or("Choisis une version du modloader.")?;
    identifier(version)?;
    if !list_loader_versions(minecraft.into(), kind.into())
        .await?
        .iter()
        .any(|v| v == version)
    {
        return Err("Cette version du modloader n’est pas disponible pour ce Minecraft.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_exact_minecraft_versions() {
        let forge = maven_versions(
            "forge",
            "1.21",
            vec!["1.21-51.0.9".into(), "1.21.1-52.0.1".into()],
        );
        assert_eq!(sorted_versions(forge), ["51.0.9"]);
        let neo = vec!["21.0.10".into(), "21.1.90".into(), "26.1.2.3-beta".into()];
        assert_eq!(
            sorted_versions(maven_versions("neoforge", "1.21", neo.clone())),
            ["21.0.10"]
        );
        assert_eq!(
            sorted_versions(maven_versions("neoforge", "26.1.2", neo.clone())),
            ["26.1.2.3-beta"]
        );
        assert!(maven_versions("neoforge", "1.19.2", neo).is_empty());
    }

    #[test]
    fn prefers_stable_versions_and_sorts_numerically() {
        let entries = ["0.10.9", "0.10.10", "0.11.0-beta.2", "../bad", "0.10.10"]
            .into_iter()
            .map(|version| MetaLoader {
                version: version.into(),
                stable: None,
            })
            .collect();
        assert_eq!(
            sorted_versions(entries),
            ["0.10.10", "0.10.9", "0.11.0-beta.2"]
        );
        let entries: Vec<MetaEntry> = serde_json::from_str(r#"[{"loader":{"version":"0.20.0","stable":false}},{"loader":{"version":"0.19.0","stable":true}}]"#).unwrap();
        assert_eq!(
            sorted_versions(entries.into_iter().map(|entry| entry.loader).collect()),
            ["0.19.0", "0.20.0"]
        );
    }

    #[tokio::test]
    #[ignore = "requires official modloader metadata services"]
    async fn official_catalogues_have_compatible_versions() {
        for kind in ["fabric", "quilt", "forge", "neoforge"] {
            let versions = list_loader_versions("1.21.1".into(), kind.into())
                .await
                .unwrap();
            assert!(!versions.is_empty(), "{kind}");
            validate_spec("1.21.1", &format!("{kind}-{}", versions[0]))
                .await
                .unwrap();
        }
        assert!(validate_spec("1.21.1", "forge-47.0.0").await.is_err());
    }
}
