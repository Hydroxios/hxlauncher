//! Shared game storage and relocation when the user chooses another folder.
use crate::{err, minecraft::safe_path, AppState, Result, Settings};
use std::path::{Path, PathBuf};

pub fn instances_root_for(app_data: &Path, settings: &Settings) -> PathBuf {
    if !settings.storage_directory.trim().is_empty() {
        PathBuf::from(&settings.storage_directory).join("instances")
    } else if !settings.instances_directory.trim().is_empty() {
        PathBuf::from(&settings.instances_directory)
    } else {
        app_data.join("instances")
    }
}

pub fn runtime_root_for(app_data: &Path, configured: &str) -> PathBuf {
    if configured.trim().is_empty() {
        app_data.to_path_buf()
    } else {
        PathBuf::from(configured).join("runtime")
    }
}

/// Upgrade the previously selected instances folder before any command can
/// resolve game paths. Persisting storage_directory makes this run only once.
pub fn initialize(state: &AppState) -> Result<()> {
    let mut settings = state.store.lock().map_err(err)?.settings.clone();
    if settings.storage_directory.trim().is_empty() {
        settings.storage_directory = if settings.instances_directory.trim().is_empty() {
            state.root.to_string_lossy().into_owned()
        } else {
            settings.instances_directory.clone()
        };
        apply_settings(state, settings)?;
    } else {
        std::fs::create_dir_all(instances_root_for(&state.root, &settings)).map_err(err)?;
        std::fs::create_dir_all(runtime_root_for(&state.root, &settings.storage_directory))
            .map_err(err)?;
    }
    Ok(())
}

fn copy_tree(source: &Path, target: &Path) -> Result<()> {
    for item in std::fs::read_dir(source).map_err(err)? {
        let item = item.map_err(err)?;
        let from = item.path();
        let to = target.join(item.file_name());
        let metadata = std::fs::symlink_metadata(&from).map_err(err)?;
        if metadata.file_type().is_symlink() {
            #[cfg(unix)]
            std::os::unix::fs::symlink(std::fs::read_link(&from).map_err(err)?, &to)
                .map_err(err)?;
            #[cfg(windows)]
            {
                let link = std::fs::read_link(&from).map_err(err)?;
                if from.is_dir() {
                    std::os::windows::fs::symlink_dir(link, &to).map_err(err)?;
                } else {
                    std::os::windows::fs::symlink_file(link, &to).map_err(err)?;
                }
            }
        } else if metadata.is_dir() {
            std::fs::create_dir(&to).map_err(err)?;
            copy_tree(&from, &to)?;
            std::fs::set_permissions(&to, metadata.permissions()).map_err(err)?;
        } else if metadata.is_file() {
            std::fs::copy(&from, &to).map_err(err)?;
            std::fs::set_permissions(&to, metadata.permissions()).map_err(err)?;
        } else {
            return Err(format!(
                "Fichier non pris en charge pendant le déplacement : {}",
                from.display()
            ));
        }
    }
    Ok(())
}

fn copy_directory(source: &Path, target: &Path, sequence: usize) -> Result<()> {
    let parent = target.parent().ok_or("Dossier de destination invalide.")?;
    std::fs::create_dir_all(parent).map_err(err)?;
    let source = std::fs::canonicalize(source).map_err(err)?;
    let parent = std::fs::canonicalize(parent).map_err(err)?;
    if parent.starts_with(&source) {
        return Err("Le nouveau dossier se trouve dans un dossier à déplacer.".into());
    }
    if target.try_exists().map_err(err)? {
        return Err(format!(
            "Le dossier de destination existe déjà : {}",
            target.display()
        ));
    }
    let stage = parent.join(format!(
        ".hx-migration-{}-{}-{sequence}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(err)?
            .as_nanos()
    ));
    std::fs::create_dir(&stage).map_err(err)?;
    let result =
        copy_tree(&source, &stage).and_then(|_| std::fs::rename(&stage, target).map_err(err));
    if result.is_err() {
        let _ = remove_within(&stage, &parent);
    }
    result
}

fn remove_within(path: &Path, parent: &Path) -> Result<()> {
    let resolved = std::fs::canonicalize(path).map_err(err)?;
    let parent = std::fs::canonicalize(parent).map_err(err)?;
    if resolved == parent || !resolved.starts_with(&parent) {
        return Err("Dossier à supprimer hors de l’emplacement attendu.".into());
    }
    std::fs::remove_dir_all(resolved).map_err(err)
}

pub fn apply_settings(state: &AppState, mut settings: Settings) -> Result<()> {
    let old = state.store.lock().map_err(err)?.clone();
    let old_instances = instances_root_for(&state.root, &old.settings);
    let old_runtime = runtime_root_for(&state.root, &old.settings.storage_directory);
    if !settings.storage_directory.trim().is_empty() {
        let selected = PathBuf::from(settings.storage_directory.trim());
        if !selected.is_absolute() {
            return Err("Choisis un dossier de stockage absolu.".into());
        }
        std::fs::create_dir_all(&selected)
            .map_err(|e| format!("Impossible de créer le dossier de stockage : {e}"))?;
        settings.storage_directory = std::fs::canonicalize(selected)
            .map_err(err)?
            .to_string_lossy()
            .into_owned();
    }
    if !settings.storage_directory.is_empty() {
        settings.instances_directory.clear();
    }
    let new_instances = instances_root_for(&state.root, &settings);
    let new_runtime = runtime_root_for(&state.root, &settings.storage_directory);
    // Validate both destination directories before publishing the new setting
    // or deleting any source data, including when there is nothing to transfer.
    std::fs::create_dir_all(&new_instances).map_err(err)?;
    std::fs::create_dir_all(&new_runtime).map_err(err)?;
    let mut moves = Vec::new();
    if old_instances != new_instances {
        for instance in &old.instances {
            let name = instance.directory.as_deref().unwrap_or(&instance.id);
            let from = safe_path(&old_instances, name)?;
            let to = safe_path(&new_instances, name)?;
            if from.try_exists().map_err(err)? {
                moves.push((from, to));
            }
        }
    }
    if old_runtime != new_runtime {
        for name in ["minecraft", "modded-runtime"] {
            let from = old_runtime.join(name);
            if from.try_exists().map_err(err)? {
                moves.push((from, new_runtime.join(name)));
            }
        }
    }
    let mut copied: Vec<PathBuf> = Vec::new();
    for (index, (from, to)) in moves.iter().enumerate() {
        if let Err(error) = copy_directory(from, to, index) {
            for path in copied.iter().rev() {
                let _ = remove_within(path, path.parent().unwrap());
            }
            return Err(format!(
                "Impossible de déplacer les données du jeu : {error}"
            ));
        }
        copied.push(to.clone());
    }
    let mut updated = old.clone();
    updated.settings = settings;
    if old_instances != new_instances {
        for instance in &mut updated.instances {
            if let Some(icon) = &instance.icon_path {
                if let Ok(relative) = Path::new(icon).strip_prefix(&old_instances) {
                    instance.icon_path =
                        Some(new_instances.join(relative).to_string_lossy().into());
                }
            }
        }
    }
    *state.store.lock().map_err(err)? = updated;
    if let Err(error) = state.save() {
        *state.store.lock().map_err(err)? = old;
        for path in copied.iter().rev() {
            let _ = remove_within(path, path.parent().unwrap());
        }
        return Err(error);
    }
    // The new layout is now the persisted source of truth. Failed cleanup only
    // leaves an unused copy; it must not undo the saved setting.
    for (from, _) in moves {
        let _ = remove_within(&from, from.parent().unwrap());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Instance, Store};
    use std::sync::Mutex;

    fn fixture() -> AppState {
        let root = std::env::temp_dir().join(format!(
            "hx-storage-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        AppState {
            root,
            store: Mutex::new(Store::default()),
            session: Mutex::new(None),
            pending: Mutex::new(None),
            busy: tokio::sync::Mutex::new(()),
        }
    }

    #[test]
    fn startup_upgrades_existing_selected_folder_and_preserves_data_after_reload() {
        let state = fixture();
        let old_instances = state.root.join("previous-instances");
        let old_game = old_instances.join("Mon Pack");
        std::fs::create_dir_all(old_game.join("saves/world")).unwrap();
        std::fs::write(old_game.join("saves/world/level.dat"), b"world").unwrap();
        std::fs::write(old_game.join("icon.png"), b"icon").unwrap();
        let old_assets = state.root.join("minecraft/assets");
        std::fs::create_dir_all(&old_assets).unwrap();
        std::fs::write(old_assets.join("test"), b"asset").unwrap();
        let old_modded = state.root.join("modded-runtime/libraries");
        std::fs::create_dir_all(&old_modded).unwrap();
        std::fs::write(old_modded.join("test"), b"library").unwrap();
        {
            let mut store = state.store.lock().unwrap();
            store.settings.instances_directory = old_instances.to_string_lossy().into();
            store.instances.push(Instance {
                id: "pack-1".into(),
                name: "Mon Pack".into(),
                version: "1.21".into(),
                loader: "fabric".into(),
                status: "Installé".into(),
                mod_count: 0,
                icon_path: Some(old_game.join("icon.png").to_string_lossy().into()),
                profile_id: Some("fabric-profile".into()),
                directory: Some("Mon Pack".into()),
            });
        }
        state.save().unwrap();
        initialize(&state).unwrap();
        let selected = std::fs::canonicalize(&old_instances).unwrap();
        assert_eq!(crate::instances_root(&state), selected.join("instances"));
        assert_eq!(crate::runtime_root(&state), selected.join("runtime"));
        let new_game = selected.join("instances/Mon Pack");
        assert_eq!(
            std::fs::read(new_game.join("saves/world/level.dat")).unwrap(),
            b"world"
        );
        assert_eq!(
            std::fs::read(selected.join("runtime/minecraft/assets/test")).unwrap(),
            b"asset"
        );
        assert_eq!(
            std::fs::read(selected.join("runtime/modded-runtime/libraries/test")).unwrap(),
            b"library"
        );
        assert!(!old_game.exists());
        assert!(!old_assets.exists());
        let saved: Store =
            serde_json::from_slice(&std::fs::read(state.root.join("state.json")).unwrap()).unwrap();
        assert_eq!(
            saved.instances[0].icon_path.as_deref(),
            new_game.join("icon.png").to_str()
        );
        assert!(saved.settings.instances_directory.is_empty());
        *state.store.lock().unwrap() = saved;
        initialize(&state).unwrap();
        assert_eq!(
            std::fs::read(new_game.join("saves/world/level.dat")).unwrap(),
            b"world"
        );
        std::fs::remove_dir_all(state.root).unwrap();
    }

    #[test]
    fn fresh_start_creates_both_storage_directories() {
        let state = fixture();
        initialize(&state).unwrap();
        assert!(state.root.join("instances").is_dir());
        assert!(state.root.join("runtime").is_dir());
        assert!(!state
            .store
            .lock()
            .unwrap()
            .settings
            .storage_directory
            .is_empty());
        std::fs::remove_dir_all(state.root).unwrap();
    }

    #[test]
    fn destination_collision_keeps_existing_configuration_and_files() {
        let state = fixture();
        let source = state.root.join("instances/pack-1");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(source.join("world.dat"), b"old").unwrap();
        state.store.lock().unwrap().instances.push(Instance {
            id: "pack-1".into(),
            name: "Pack".into(),
            version: "1.21".into(),
            loader: "Vanilla".into(),
            status: "Installé".into(),
            mod_count: 0,
            icon_path: None,
            profile_id: None,
            directory: None,
        });
        let selected = state.root.join("chosen");
        let target = selected.join("instances/pack-1");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("world.dat"), b"other").unwrap();
        let settings = Settings {
            storage_directory: selected.to_string_lossy().into(),
            ..Settings::default()
        };
        assert!(apply_settings(&state, settings).is_err());
        assert_eq!(std::fs::read(source.join("world.dat")).unwrap(), b"old");
        assert_eq!(std::fs::read(target.join("world.dat")).unwrap(), b"other");
        assert!(state
            .store
            .lock()
            .unwrap()
            .settings
            .storage_directory
            .is_empty());
        std::fs::remove_dir_all(state.root).unwrap();
    }
}
