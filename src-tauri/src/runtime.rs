//! Install Mojang's raw Java files: mc-launcher-core 0.1.2 incorrectly decodes
//! the alternative LZMA downloads as XZ streams.
use crate::{
    err, http,
    minecraft::{download, safe_path},
    Result,
};
use serde_json::Value;
use std::path::{Component, Path, PathBuf};

const MANIFEST: &str = "https://launchermeta.mojang.com/v1/products/java-runtime/2ec0cc96c44e5a76b9c8b7c39df7210883d12871/all.json";

/// Keep Java's redirected stdout/stderr without creating a Windows console.
/// CREATE_NO_WINDOW does not hide Minecraft's own graphical window.
pub fn command(executable: impl AsRef<std::ffi::OsStr>) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(executable);
    command.stdin(std::process::Stdio::null());
    #[cfg(target_os = "windows")]
    command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    command
}

fn platform() -> Result<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Ok("mac-os-arm64"),
        ("macos", "x86_64") => Ok("mac-os"),
        ("windows", "x86") => Ok("windows-x86"),
        ("windows", "x86_64") => Ok("windows-x64"),
        ("windows", "aarch64") => Ok("windows-arm64"),
        ("linux", "x86") => Ok("linux-i386"),
        ("linux", "x86_64") => Ok("linux"),
        _ => Err("Aucun runtime Mojang compatible avec cette architecture.".into()),
    }
}

fn executable(base: &Path) -> Option<PathBuf> {
    [
        "bin/java",
        "bin/java.exe",
        "jre.bundle/Contents/Home/bin/java",
    ]
    .iter()
    .map(|p| base.join(p))
    .find(|p| p.is_file())
}

// Resolve relative symlink targets while allowing ../ within the runtime.
fn link_target(base: &Path, name: &str, target: &str) -> Result<PathBuf> {
    if target.is_empty() || target.contains(['\\', ':']) {
        return Err("Lien Java invalide.".into());
    }
    let mut relative = PathBuf::from(name)
        .parent()
        .ok_or("Lien Java invalide.")?
        .to_path_buf();
    for part in Path::new(target).components() {
        match part {
            Component::Normal(p) => relative.push(p),
            Component::CurDir => {}
            Component::ParentDir if relative.pop() => {}
            _ => return Err("Lien Java hors du runtime.".into()),
        }
    }
    Ok(base.join(relative))
}

pub async fn ensure(
    root: &Path,
    component: &str,
    mut progress: impl FnMut(usize, usize),
) -> Result<PathBuf> {
    crate::modded::identifier(component)?;
    // Separate from the old installer, whose mere bin/java existence check
    // could mistakenly accept an interrupted installation.
    let base = root.join("java-runtime").join(platform()?).join(component);
    let complete = base.join(".hx-complete");
    if complete.is_file() {
        if let Some(java) = executable(&base) {
            return Ok(java);
        }
    }
    let index: Value = http()
        .get(MANIFEST)
        .send()
        .await
        .map_err(err)?
        .error_for_status()
        .map_err(err)?
        .json()
        .await
        .map_err(err)?;
    let entry = &index[platform()?][component][0]["manifest"];
    let manifest_path = base.join(".manifest.json");
    download(
        entry["url"]
            .as_str()
            .ok_or("Runtime Java indisponible pour cette plateforme.")?,
        &manifest_path,
        Some(
            entry["sha1"]
                .as_str()
                .ok_or("SHA-1 du manifeste Java absent.")?,
        ),
        8 * 1024 * 1024,
    )
    .await?;
    let manifest: Value =
        serde_json::from_slice(&tokio::fs::read(&manifest_path).await.map_err(err)?)
            .map_err(err)?;
    let files = manifest["files"]
        .as_object()
        .ok_or("Manifeste Java invalide.")?;
    let mut links = Vec::new();
    for (i, (name, entry)) in files.iter().enumerate() {
        let path = safe_path(&base, name)?;
        match entry["type"].as_str() {
            Some("directory") => tokio::fs::create_dir_all(&path).await.map_err(err)?,
            Some("file") => {
                let raw = &entry["downloads"]["raw"];
                download(
                    raw["url"].as_str().ok_or("Fichier Java brut absent.")?,
                    &path,
                    Some(raw["sha1"].as_str().ok_or("SHA-1 Java absent.")?),
                    512 * 1024 * 1024,
                )
                .await
                .map_err(|e| format!("{name} : {e}"))?;
                #[cfg(unix)]
                if entry["executable"] == true {
                    use std::os::unix::fs::PermissionsExt;
                    tokio::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                        .await
                        .map_err(err)?;
                }
            }
            Some("link") => {
                let target = entry["target"]
                    .as_str()
                    .ok_or("Cible du lien Java absente.")?;
                let resolved = link_target(&base, name, target)?;
                links.push((path, target.to_owned(), resolved));
            }
            _ => return Err(format!("Type de fichier Java inconnu : {name}")),
        }
        progress(i + 1, files.len());
    }
    for (path, target, _resolved) in links {
        tokio::fs::create_dir_all(path.parent().ok_or("Lien Java invalide.")?)
            .await
            .map_err(err)?;
        if tokio::fs::symlink_metadata(&path).await.is_ok() {
            tokio::fs::remove_file(&path).await.map_err(err)?;
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, &path).map_err(err)?;
        #[cfg(windows)]
        {
            let _ = target;
            tokio::fs::copy(_resolved, &path).await.map_err(err)?;
        }
    }
    let java = executable(&base).ok_or("Le runtime Java téléchargé est incomplet.")?;
    tokio::fs::write(complete, b"raw-sha1-v1")
        .await
        .map_err(err)?;
    Ok(java)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn runtime_links_stay_within_installation() {
        let base = Path::new("runtime");
        assert_eq!(
            link_target(base, "legal/module/LICENSE", "../java.base/LICENSE").unwrap(),
            base.join("legal/java.base/LICENSE")
        );
        for target in ["../../../escape", "/tmp/escape", "C:\\escape"] {
            assert!(link_target(base, "legal/module/LICENSE", target).is_err());
        }
    }
    #[tokio::test]
    #[ignore = "downloads the official Java runtime and runs java -version"]
    async fn official_runtime_installs_and_resumes() {
        let root = std::env::temp_dir().join(format!("hx-java-smoke-{}", std::process::id()));
        let java = ensure(&root, "java-runtime-delta", |_, _| {})
            .await
            .unwrap();
        let output = tokio::process::Command::new(&java)
            .arg("-version")
            .output()
            .await
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        // Simulate an interrupted install with bin/java already present.
        let base = root
            .join("java-runtime")
            .join(platform().unwrap())
            .join("java-runtime-delta");
        tokio::fs::remove_file(base.join(".hx-complete"))
            .await
            .unwrap();
        tokio::fs::write(&java, b"interrupted").await.unwrap();
        assert_eq!(
            ensure(&root, "java-runtime-delta", |_, _| {})
                .await
                .unwrap(),
            java
        );
        assert!(tokio::process::Command::new(&java)
            .arg("-version")
            .output()
            .await
            .unwrap()
            .status
            .success());
        assert_eq!(
            ensure(&root, "java-runtime-delta", |_, _| panic!(
                "completed runtime should be reused"
            ))
            .await
            .unwrap(),
            java
        );
        tokio::fs::remove_dir_all(root).await.unwrap();
    }
}
