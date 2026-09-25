//! Stable instance IDs are independent of their human-readable directory names.
use crate::{err, instances_root, minecraft::safe_path, AppState, Result};
use std::path::PathBuf;

fn folder_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_control() || "<>:\"/\\|?*".contains(c) {
                '_'
            } else {
                c
            }
        })
        .take(80)
        .collect();
    let mut cleaned = cleaned;
    while cleaned.len() > 180 {
        cleaned.pop();
    }
    let cleaned = cleaned.trim().trim_matches('.').trim();
    if cleaned.is_empty() {
        return "Instance".into();
    }
    let stem = cleaned
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'))
    {
        format!("_{cleaned}")
    } else {
        cleaned.into()
    }
}

pub fn available_name(state: &AppState, name: &str) -> Result<String> {
    let root = instances_root(state);
    let mut used: std::collections::HashSet<String> = state
        .store
        .lock()
        .map_err(err)?
        .instances
        .iter()
        .map(|i| i.directory.as_deref().unwrap_or(&i.id).to_lowercase())
        .collect();
    match std::fs::read_dir(&root) {
        Ok(entries) => {
            for entry in entries {
                used.insert(
                    entry
                        .map_err(err)?
                        .file_name()
                        .to_string_lossy()
                        .to_lowercase(),
                );
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(err(e)),
    }
    let base = folder_name(name);
    let mut candidate = base.clone();
    let mut suffix = 2;
    while used.contains(&candidate.to_lowercase()) {
        candidate = format!("{base} ({suffix})");
        suffix += 1;
    }
    Ok(candidate)
}

/// Called while the launcher's operation lock is held. Migrate legacy folders
/// on first use, preserving saves, mods, configs and icon paths.
pub fn directory(state: &AppState, id: &str) -> Result<PathBuf> {
    let original = state
        .store
        .lock()
        .map_err(err)?
        .instances
        .iter()
        .find(|i| i.id == id)
        .cloned()
        .ok_or("Instance introuvable.")?;
    let root = instances_root(state);
    if let Some(directory) = &original.directory {
        return safe_path(&root, directory);
    }
    let name = available_name(state, &original.name)?;
    let old = safe_path(&root, &original.id)?;
    let target = safe_path(&root, &name)?;
    let moved = old.try_exists().map_err(err)?;
    if moved {
        std::fs::rename(&old, &target)
            .map_err(|e| format!("Impossible de renommer le dossier de l’instance : {e}"))?;
    }
    {
        let mut store = state.store.lock().map_err(err)?;
        let instance = store
            .instances
            .iter_mut()
            .find(|i| i.id == id)
            .ok_or("Instance introuvable.")?;
        instance.directory = Some(name);
        if let Some(icon) = &original.icon_path {
            if let Ok(relative) = std::path::Path::new(icon).strip_prefix(&old) {
                instance.icon_path = Some(target.join(relative).to_string_lossy().into());
            }
        }
    }
    if let Err(error) = state.save() {
        if moved {
            std::fs::rename(&target, &old).map_err(|e| {
                format!(
                    "{error}. Impossible de restaurer le dossier {} : {e}",
                    old.display()
                )
            })?;
        }
        if let Some(instance) = state
            .store
            .lock()
            .map_err(err)?
            .instances
            .iter_mut()
            .find(|i| i.id == id)
        {
            *instance = original;
        }
        return Err(error);
    }
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Instance, Store};
    use std::sync::Mutex;

    fn state() -> AppState {
        let root = std::env::temp_dir().join(format!(
            "hx-instance-test-{}-{}",
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
    fn legacy(state: &AppState) -> PathBuf {
        let path = instances_root(state).join("pack-123");
        std::fs::create_dir_all(path.join("saves/Mon monde")).unwrap();
        std::fs::write(path.join("saves/Mon monde/level.dat"), b"world data").unwrap();
        std::fs::write(path.join("icon.png"), b"icon").unwrap();
        state.store.lock().unwrap().instances.push(Instance {
            id: "pack-123".into(),
            name: "Mon Pack été".into(),
            version: "1.21.1".into(),
            loader: "fabric".into(),
            status: "Installé".into(),
            mod_count: 1,
            icon_path: Some(path.join("icon.png").to_string_lossy().into()),
            profile_id: Some("fabric-profile".into()),
            directory: None,
        });
        state.save().unwrap();
        path
    }
    #[test]
    fn names_are_readable_safe_and_unique() {
        assert_eq!(folder_name("  Mon Pack été  "), "Mon Pack été");
        assert_eq!(folder_name("../A:B\\C?"), "_A_B_C_");
        assert_eq!(folder_name("..."), "Instance");
        assert_eq!(folder_name("CON.txt"), "_CON.txt");
        let state = state();
        legacy(&state);
        std::fs::create_dir_all(instances_root(&state).join("mon pack été")).unwrap();
        assert_eq!(
            available_name(&state, "Mon Pack été").unwrap(),
            "Mon Pack été (2)"
        );
        assert!(directory(&state, "unknown").is_err());
        std::fs::remove_dir_all(state.root).unwrap();
    }
    #[test]
    fn migration_preserves_worlds_icons_ids_and_survives_reload() {
        let state = state();
        let old = legacy(&state);
        let target = directory(&state, "pack-123").unwrap();
        assert_eq!(target.file_name().unwrap(), "Mon Pack été");
        assert!(!old.exists());
        assert_eq!(
            std::fs::read(target.join("saves/Mon monde/level.dat")).unwrap(),
            b"world data"
        );
        let saved: Store =
            serde_json::from_slice(&std::fs::read(state.root.join("state.json")).unwrap()).unwrap();
        assert_eq!(saved.instances[0].id, "pack-123");
        assert_eq!(
            saved.instances[0].icon_path.as_deref(),
            target.join("icon.png").to_str()
        );
        *state.store.lock().unwrap() = saved;
        assert_eq!(directory(&state, "pack-123").unwrap(), target);
        assert_eq!(
            available_name(&state, "Mon Pack été").unwrap(),
            "Mon Pack été (2)"
        );
        std::fs::remove_dir_all(state.root).unwrap();
    }
    #[test]
    fn failed_save_rolls_back_migration() {
        let state = state();
        let old = legacy(&state);
        std::fs::create_dir(state.root.join("state.tmp")).unwrap();
        assert!(directory(&state, "pack-123").is_err());
        assert!(old.join("saves/Mon monde/level.dat").is_file());
        assert!(state.store.lock().unwrap().instances[0].directory.is_none());
        std::fs::remove_dir_all(state.root).unwrap();
    }
}
