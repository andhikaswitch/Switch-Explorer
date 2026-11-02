//! `SettingsStore`: value layers, cheap snapshots, Hot Switch semantics and `settings.toml`.
//!
//! Layers (DEVELOPER-MODE.md §4): defaults → `settings.toml` → session (`--set`, non-persistent
//! Hot Switch). Presets and Location Rules join in later phases. Every change produces a
//! [`Change`] so the UI can update the status chip and show the OSD (CLAUDE.md rule 4).

use std::collections::BTreeMap;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use sx_core::ChangeSource;
use toml_edit::{DocumentMut, Item};

use crate::error::SettingsError;
use crate::flags::{FLAGS, Features, meta};
use crate::meta::{FlagMeta, Stability};
use crate::value::{FlagKind, FlagValue};

/// Version of the `settings.toml` layout; bumped together with a migration.
pub const SCHEMA: i64 = 1;

/// One switch that changed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub id: &'static str,
    pub previous: FlagValue,
    pub value: FlagValue,
    pub source: ChangeSource,
    /// False when the change only lives in the session layer.
    pub persisted: bool,
}

#[derive(Debug)]
pub struct SettingsStore {
    path: Option<PathBuf>,
    doc: DocumentMut,
    /// Defaults + `settings.toml` + persisted changes.
    persisted: Features,
    session: BTreeMap<&'static str, FlagValue>,
    snapshot: Arc<Features>,
    dirty: bool,
}

impl SettingsStore {
    /// Defaults only, never written to disk (tests, `--safe-mode`).
    pub fn in_memory() -> Self {
        Self::from_parts(None, DocumentMut::new(), Features::default())
    }

    /// Loads `settings.toml`. A missing file means defaults. Invalid values fall back to their
    /// default and are returned as warnings. A file that is not TOML falls back to `.bak`.
    pub fn load(path: &Path) -> Result<(Self, Vec<SettingsError>), SettingsError> {
        let doc = match read_doc(path) {
            Ok(doc) => doc,
            Err(err @ SettingsError::Parse { .. }) => {
                let backup = backup_path(path);
                tracing::warn!(error = %err, "settings.toml is broken, trying the backup");
                read_doc(&backup).map_err(|_| err)?
            }
            Err(err) => return Err(err),
        };
        let mut features = Features::default();
        let mut warnings = Vec::new();
        for (key, item) in doc.as_table() {
            let (Some(meta), Some(value)) = (meta(key), item.as_value()) else {
                continue; // `schema`, `[[rules]]`, unknown keys: preserved untouched.
            };
            match meta
                .kind
                .from_toml(meta.id, value)
                .and_then(|v| features.set(meta.id, &v))
            {
                Ok(()) => {}
                Err(err) => warnings.push(err),
            }
        }
        Ok((
            Self::from_parts(Some(path.to_path_buf()), doc, features),
            warnings,
        ))
    }

    fn from_parts(path: Option<PathBuf>, doc: DocumentMut, persisted: Features) -> Self {
        Self {
            path,
            doc,
            snapshot: Arc::new(persisted.clone()),
            persisted,
            session: BTreeMap::new(),
            dirty: false,
        }
    }

    /// The effective values. Cheap to clone; replaced (not mutated) on every change.
    pub fn snapshot(&self) -> Arc<Features> {
        Arc::clone(&self.snapshot)
    }

    pub fn get(&self, id: &str) -> Option<FlagValue> {
        self.snapshot.get(id)
    }

    /// Whether there are persisted changes not yet written to disk.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Sets a switch. CLI changes stay in the session; Hot Switch changes persist only when
    /// `hotswitch.persist` is on; everything else persists.
    pub fn set(
        &mut self,
        id: &str,
        value: FlagValue,
        source: ChangeSource,
    ) -> Result<Change, SettingsError> {
        let meta = meta(id).ok_or_else(|| SettingsError::UnknownFlag(id.to_string()))?;
        let value = meta.kind.check(meta.id, value)?;
        self.guard(meta, &value, source)?;
        let persist = match source {
            ChangeSource::Cli => false,
            ChangeSource::HotSwitch => self.snapshot.hotswitch_persist,
            _ => true,
        };
        let previous = self.effective(meta.id);
        if persist {
            self.persisted.set(meta.id, &value)?;
            // An explicit, persisted choice replaces any session override of the same switch.
            self.session.remove(meta.id);
            self.dirty = true;
        } else {
            self.session.insert(meta.id, value.clone());
        }
        self.rebuild();
        Ok(Change {
            id: meta.id,
            previous,
            value,
            source,
            persisted: persist,
        })
    }

    /// Hot Switch toggle: bools flip, enums step to the next variant.
    pub fn toggle(&mut self, id: &str, source: ChangeSource) -> Result<Change, SettingsError> {
        let meta = meta(id).ok_or_else(|| SettingsError::UnknownFlag(id.to_string()))?;
        let current = self.effective(meta.id);
        let next = match (meta.kind, &current) {
            (FlagKind::Bool, FlagValue::Bool(b)) => FlagValue::Bool(!b),
            (FlagKind::Enum(variants), FlagValue::Text(s)) => {
                let ix = variants.iter().position(|v| v == s).unwrap_or(0);
                FlagValue::Text(variants[(ix + 1) % variants.len()].to_string())
            }
            _ => return Err(SettingsError::NotHotSwitchable(meta.id)),
        };
        self.set(meta.id, next, source)
    }

    /// Hot Switch group: if any member is on, all go off; if all are off, all go on.
    pub fn toggle_group(
        &mut self,
        ids: &[&str],
        source: ChangeSource,
    ) -> Result<Vec<Change>, SettingsError> {
        let any_on = ids
            .iter()
            .any(|id| self.get(id) == Some(FlagValue::Bool(true)));
        ids.iter()
            .map(|id| self.set(id, FlagValue::Bool(!any_on), source))
            .collect()
    }

    /// Applies `--set id=value` to the session layer.
    pub fn apply_cli(&mut self, assignment: &str) -> Result<Change, SettingsError> {
        let (id, raw) = assignment
            .split_once('=')
            .ok_or_else(|| SettingsError::BadAssignment(assignment.to_string()))?;
        let meta = meta(id.trim()).ok_or_else(|| SettingsError::UnknownFlag(id.to_string()))?;
        let value = meta.kind.parse_cli(meta.id, raw.trim())?;
        self.set(meta.id, value, ChangeSource::Cli)
    }

    /// Writes `settings.toml`: only values that differ from the default, unknown keys and
    /// comments preserved, atomic replace with a `.bak` of the previous file.
    pub fn save(&mut self) -> Result<(), SettingsError> {
        let Some(path) = self.path.clone() else {
            self.dirty = false;
            return Ok(());
        };
        let defaults = Features::default();
        self.doc["schema"] = toml_edit::value(SCHEMA);
        for meta in FLAGS {
            let value = self.persisted.get(meta.id);
            if value == defaults.get(meta.id) {
                self.doc.as_table_mut().remove(meta.id);
            } else if let Some(value) = value {
                self.doc
                    .as_table_mut()
                    .insert(meta.id, Item::Value(value.to_toml()));
            }
        }
        write_atomic(&path, self.doc.to_string().as_bytes())?;
        self.dirty = false;
        Ok(())
    }

    fn effective(&self, id: &str) -> FlagValue {
        self.session
            .get(id)
            .cloned()
            .or_else(|| self.persisted.get(id))
            .unwrap_or(FlagValue::Bool(false))
    }

    /// Experimental switches can only be turned on in Developer Mode, and Hot Switch only
    /// touches switches marked `hotswitchable` (DEVELOPER-MODE.md §6.4).
    fn guard(
        &self,
        meta: &'static FlagMeta,
        value: &FlagValue,
        source: ChangeSource,
    ) -> Result<(), SettingsError> {
        if source == ChangeSource::HotSwitch && !meta.hotswitchable {
            return Err(SettingsError::NotHotSwitchable(meta.id));
        }
        let enabling = *value != FlagValue::Bool(false);
        let interactive = matches!(source, ChangeSource::User | ChangeSource::HotSwitch);
        if interactive
            && enabling
            && meta.stability == Stability::Experimental
            && !self.snapshot.dev_mode
        {
            return Err(SettingsError::NeedsDeveloperMode(meta.id));
        }
        Ok(())
    }

    fn rebuild(&mut self) {
        let mut effective = self.persisted.clone();
        for (id, value) in &self.session {
            // Session values were validated when they were stored.
            let _ = effective.set(id, value);
        }
        self.snapshot = Arc::new(effective);
    }
}

fn read_doc(path: &Path) -> Result<DocumentMut, SettingsError> {
    match fs::read_to_string(path) {
        Ok(text) => text
            .parse::<DocumentMut>()
            .map_err(|err| SettingsError::Parse {
                path: path.to_path_buf(),
                message: err.to_string(),
            }),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(DocumentMut::new()),
        Err(source) => Err(SettingsError::Read {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn backup_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".bak");
    PathBuf::from(name)
}

/// Temp file in the same folder → flush → keep the old file as `.bak` → rename over the target.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), SettingsError> {
    let write_err = |source| SettingsError::Write {
        path: path.to_path_buf(),
        source,
    };
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(write_err)?;
    }
    let mut tmp_name = path.as_os_str().to_owned();
    tmp_name.push(".tmp");
    let tmp = PathBuf::from(tmp_name);
    {
        let mut file = fs::File::create(&tmp).map_err(write_err)?;
        file.write_all(bytes).map_err(write_err)?;
        file.sync_all().map_err(write_err)?;
    }
    if path.exists() {
        fs::copy(path, backup_path(path)).map_err(write_err)?;
    }
    fs::rename(&tmp, path).map_err(write_err)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store_in(dir: &tempfile::TempDir, text: &str) -> (SettingsStore, Vec<SettingsError>) {
        let path = dir.path().join("settings.toml");
        fs::write(&path, text).unwrap();
        SettingsStore::load(&path).unwrap()
    }

    #[test]
    fn missing_file_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let (store, warnings) = SettingsStore::load(&dir.path().join("settings.toml")).unwrap();
        assert!(warnings.is_empty());
        assert_eq!(*store.snapshot(), Features::default());
    }

    #[test]
    fn invalid_values_warn_and_keep_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let (store, warnings) = store_in(
            &dir,
            "\"dnd.enabled\" = false\n\"dnd.scroll_cooldown_ms\" = 99999\n\"view.show_hidden\" = \"yes\"\n",
        );
        assert_eq!(warnings.len(), 2);
        let f = store.snapshot();
        assert!(!f.dnd_enabled);
        assert_eq!(f.dnd_scroll_cooldown_ms, 0);
        assert!(!f.view_show_hidden);
    }

    #[test]
    fn save_writes_only_changes_and_keeps_unknown_keys() {
        let dir = tempfile::tempdir().unwrap();
        let text = "# my comment\n\"future.flag\" = 7\n\n[[rules]]\npath = 'D:\\Kuliah\\**'\n\"dnd.source.folders\" = false\n";
        let (mut store, _) = store_in(&dir, text);
        store
            .set(
                "view.show_hidden",
                FlagValue::Bool(true),
                ChangeSource::User,
            )
            .unwrap();
        store
            .set("dnd.enabled", FlagValue::Bool(true), ChangeSource::User)
            .unwrap();
        store.save().unwrap();
        let saved = fs::read_to_string(dir.path().join("settings.toml")).unwrap();
        assert!(saved.contains("# my comment"));
        assert!(saved.contains("\"future.flag\" = 7"));
        assert!(saved.contains("[[rules]]"));
        assert!(saved.contains("\"view.show_hidden\" = true"));
        assert!(
            !saved.contains("\"dnd.enabled\""),
            "defaults are not written"
        );
        assert!(saved.contains("schema = 1"));
        assert!(dir.path().join("settings.toml.bak").exists());
        // Round trip.
        let (reloaded, warnings) = SettingsStore::load(&dir.path().join("settings.toml")).unwrap();
        assert!(warnings.is_empty());
        assert!(reloaded.snapshot().view_show_hidden);
    }

    #[test]
    fn broken_file_falls_back_to_backup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.toml");
        fs::write(&path, "\"view.show_hidden\" = true\n").unwrap();
        fs::write(backup_path(&path), "\"view.show_hidden\" = true\n").unwrap();
        fs::write(&path, "this is = = not toml").unwrap();
        let (store, _) = SettingsStore::load(&path).unwrap();
        assert!(store.snapshot().view_show_hidden);
    }

    #[test]
    fn cli_is_session_only() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, _) = store_in(&dir, "");
        let change = store.apply_cli("dnd.scroll_cooldown_ms=400").unwrap();
        assert!(!change.persisted);
        assert_eq!(store.snapshot().dnd_scroll_cooldown_ms, 400);
        store.save().unwrap();
        let saved = fs::read_to_string(dir.path().join("settings.toml")).unwrap();
        assert!(!saved.contains("scroll_cooldown"));
        assert!(store.apply_cli("nonsense").is_err());
        assert!(store.apply_cli("dnd.enabled=maybe").is_err());
    }

    #[test]
    fn hotswitch_toggle_respects_persist_and_metadata() {
        let mut store = SettingsStore::in_memory();
        let change = store
            .toggle("dnd.source.folders", ChangeSource::HotSwitch)
            .unwrap();
        assert_eq!(change.previous, FlagValue::Bool(true));
        assert_eq!(change.value, FlagValue::Bool(false));
        assert!(change.persisted);
        // Not hotswitchable.
        assert!(matches!(
            store.toggle("ops.undo", ChangeSource::HotSwitch),
            Err(SettingsError::NotHotSwitchable(_))
        ));
        // Session-only when hotswitch.persist is off.
        store
            .set(
                "hotswitch.persist",
                FlagValue::Bool(false),
                ChangeSource::User,
            )
            .unwrap();
        let change = store
            .toggle("view.show_hidden", ChangeSource::HotSwitch)
            .unwrap();
        assert!(!change.persisted);
        assert!(store.snapshot().view_show_hidden);
    }

    #[test]
    fn enum_cycles() {
        let mut store = SettingsStore::in_memory();
        let order: Vec<_> = (0..4)
            .map(|_| {
                store
                    .toggle("dnd.default_action", ChangeSource::HotSwitch)
                    .unwrap()
                    .value
                    .to_string()
            })
            .collect();
        assert_eq!(order, ["copy", "move", "ask", "system"]);
    }

    #[test]
    fn group_toggle() {
        let mut store = SettingsStore::in_memory();
        let ids = ["dnd.external_out", "dnd.external_in"];
        store
            .set(
                "dnd.external_in",
                FlagValue::Bool(false),
                ChangeSource::User,
            )
            .unwrap();
        // One is still on → all go off.
        store.toggle_group(&ids, ChangeSource::HotSwitch).unwrap();
        assert!(!store.snapshot().dnd_external_out && !store.snapshot().dnd_external_in);
        // All off → all go on.
        store.toggle_group(&ids, ChangeSource::HotSwitch).unwrap();
        assert!(store.snapshot().dnd_external_out && store.snapshot().dnd_external_in);
    }

    #[test]
    fn experimental_needs_developer_mode() {
        let mut store = SettingsStore::in_memory();
        let on = FlagValue::Bool(true);
        assert!(matches!(
            store.set(
                "integration.picker_takeover",
                on.clone(),
                ChangeSource::User
            ),
            Err(SettingsError::NeedsDeveloperMode(_))
        ));
        store
            .set("dev.mode", FlagValue::Bool(true), ChangeSource::User)
            .unwrap();
        assert!(
            store
                .set("integration.picker_takeover", on, ChangeSource::User)
                .is_ok()
        );
    }

    #[test]
    fn snapshots_are_immutable() {
        let mut store = SettingsStore::in_memory();
        let before = store.snapshot();
        store
            .set(
                "view.show_hidden",
                FlagValue::Bool(true),
                ChangeSource::User,
            )
            .unwrap();
        assert!(!before.view_show_hidden);
        assert!(store.snapshot().view_show_hidden);
    }
}
