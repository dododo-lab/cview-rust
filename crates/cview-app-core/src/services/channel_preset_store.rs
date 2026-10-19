//! `Sources/CViewApp/Services/ChannelPresetStore.swift` 포팅.
//!
//! 멀티채팅·멀티라이브 채널 묶음 프리셋. Swift 의 전역 액터(`ChannelPresetIO`)가 하던 파일
//! 트랜잭션 직렬화는 프로세스 전역 `Mutex` 가 맡는다 — 캐시와 디스크 쓰기가 같은 잠금 안이다.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::support::{lock, write_atomic};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum PresetKind {
    /// 멀티채팅 채널 묶음.
    #[default]
    Chat,
    /// 멀티라이브 채널 묶음.
    Live,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Preset {
    pub id: Uuid,
    pub name: String,
    pub channels: Vec<String>,
    /// 키가 없는 기존 파일은 채팅 묶음으로 읽는다.
    #[serde(default)]
    pub kind: PresetKind,
}

impl Preset {
    pub fn new(name: impl Into<String>, channels: Vec<String>, kind: PresetKind) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            channels,
            kind,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct Envelope {
    version: i64,
    presets: Vec<Preset>,
}

#[derive(Debug, thiserror::Error)]
pub enum PresetStoreError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("decode: {0}")]
    Decode(#[from] serde_json::Error),
    /// `CocoaError(.fileReadCorruptFile)` — 지원하지 않는 버전.
    #[error("지원하지 않는 프리셋 파일 버전")]
    UnsupportedVersion,
}

fn caches() -> &'static Mutex<HashMap<PathBuf, Vec<Preset>>> {
    static CACHES: OnceLock<Mutex<HashMap<PathBuf, Vec<Preset>>>> = OnceLock::new();
    CACHES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 채널 묶음 프리셋 저장소.
#[derive(Debug, Clone)]
pub struct ChannelPresetStore {
    file: PathBuf,
}

impl ChannelPresetStore {
    pub fn new(file: impl Into<PathBuf>) -> Self {
        Self { file: file.into() }
    }

    /// `<app support>/CView/multichat-presets.json`.
    pub fn default_path(app_support: &Path) -> PathBuf {
        app_support.join("CView").join("multichat-presets.json")
    }

    fn load_locked(
        &self,
        cache: &mut HashMap<PathBuf, Vec<Preset>>,
    ) -> Result<Vec<Preset>, PresetStoreError> {
        if let Some(cached) = cache.get(&self.file) {
            return Ok(cached.clone());
        }
        let presets = if self.file.exists() {
            let bytes = std::fs::read(&self.file)?;
            Self::decode(&bytes)?.presets
        } else {
            Vec::new()
        };
        cache.insert(self.file.clone(), presets.clone());
        Ok(presets)
    }

    pub fn load_all(&self) -> Result<Vec<Preset>, PresetStoreError> {
        let mut cache = lock(caches());
        self.load_locked(&mut cache)
    }

    pub fn save(&self, preset: &Preset) -> Result<(), PresetStoreError> {
        let mut cache = lock(caches());
        let mut existing = self.load_locked(&mut cache)?;
        Self::merge(preset, &mut existing);
        self.persist(existing, &mut cache)
    }

    fn unique_name(base: &str, taken: impl Fn(&str) -> bool) -> String {
        let mut name = base.to_string();
        let mut i = 1;
        while taken(&name) {
            name = format!("{base} ({i})");
            i += 1;
        }
        name
    }

    fn merge(preset: &Preset, existing: &mut Vec<Preset>) {
        let name = Self::unique_name(&preset.name, |n| {
            existing.iter().any(|p| p.name == n && p.id != preset.id)
        });
        let normalized = Preset {
            id: preset.id,
            name,
            channels: preset.channels.clone(),
            kind: preset.kind,
        };
        existing.retain(|p| p.id != preset.id);
        existing.push(normalized);
    }

    pub fn delete(&self, id: Uuid) -> Result<(), PresetStoreError> {
        let mut cache = lock(caches());
        let mut existing = self.load_locked(&mut cache)?;
        existing.retain(|p| p.id != id);
        self.persist(existing, &mut cache)
    }

    /// 삭제된 프리셋을 원래 자리로 되돌린다(실행 취소).
    pub fn restore(&self, preset: &Preset, index: usize) -> Result<(), PresetStoreError> {
        let mut cache = lock(caches());
        let mut existing = self.load_locked(&mut cache)?;
        existing.retain(|p| p.id != preset.id);
        let name = Self::unique_name(&preset.name, |n| existing.iter().any(|p| p.name == n));
        let normalized = Preset {
            id: preset.id,
            name,
            channels: preset.channels.clone(),
            kind: preset.kind,
        };
        let at = index.min(existing.len());
        existing.insert(at, normalized);
        self.persist(existing, &mut cache)
    }

    pub fn export_all(&self, to: &Path) -> Result<(), PresetStoreError> {
        let env = Envelope {
            version: 1,
            presets: self.load_all()?,
        };
        write_atomic(to, &serde_json::to_vec(&env)?)?;
        Ok(())
    }

    pub fn import_from_file(&self, at: &Path) -> Result<(), PresetStoreError> {
        let bytes = std::fs::read(at)?;
        let env = Self::decode(&bytes)?;
        let mut cache = lock(caches());
        let mut existing = self.load_locked(&mut cache)?;
        for preset in &env.presets {
            Self::merge(preset, &mut existing);
        }
        self.persist(existing, &mut cache)
    }

    fn decode(bytes: &[u8]) -> Result<Envelope, PresetStoreError> {
        let env: Envelope = serde_json::from_slice(bytes)?;
        if env.version != 1 {
            return Err(PresetStoreError::UnsupportedVersion);
        }
        Ok(env)
    }

    fn persist(
        &self,
        presets: Vec<Preset>,
        cache: &mut HashMap<PathBuf, Vec<Preset>>,
    ) -> Result<(), PresetStoreError> {
        let env = Envelope {
            version: 1,
            presets,
        };
        write_atomic(&self.file, &serde_json::to_vec(&env)?)?;
        cache.insert(self.file.clone(), env.presets);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    //! 원본: `Tests/CViewAppTests/ChannelPresetStoreTests.swift`
    use super::*;

    fn temp_file() -> PathBuf {
        std::env::temp_dir()
            .join(format!("cview-presets-{}", Uuid::new_v4()))
            .join("p.json")
    }

    fn preset(name: &str, channels: &[&str]) -> Preset {
        Preset::new(
            name,
            channels.iter().map(|s| s.to_string()).collect(),
            PresetKind::Chat,
        )
    }

    #[test]
    fn save_load_round_trip() {
        let store = ChannelPresetStore::new(temp_file());
        store.save(&preset("RPG", &["c1", "c2"])).unwrap();
        let loaded = store.load_all().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "RPG");
        assert_eq!(loaded[0].channels, vec!["c1", "c2"]);
    }

    #[test]
    fn delete() {
        let store = ChannelPresetStore::new(temp_file());
        let a = preset("A", &["1"]);
        store.save(&a).unwrap();
        store.save(&preset("B", &["2"])).unwrap();
        store.delete(a.id).unwrap();
        let loaded = store.load_all().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "B");
    }

    #[test]
    fn export_import_round_trip() {
        let a = ChannelPresetStore::new(temp_file());
        a.save(&preset("X", &["c"])).unwrap();
        let export = temp_file();
        a.export_all(&export).unwrap();
        let b = ChannelPresetStore::new(temp_file());
        b.import_from_file(&export).unwrap();
        let loaded = b.load_all().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "X");
    }

    #[test]
    fn name_collision_gets_suffix() {
        let store = ChannelPresetStore::new(temp_file());
        store.save(&preset("Same", &[])).unwrap();
        store.save(&preset("Same", &[])).unwrap();
        let names: Vec<String> = store
            .load_all()
            .unwrap()
            .into_iter()
            .map(|p| p.name)
            .collect();
        assert_eq!(names, vec!["Same", "Same (1)"]);
    }

    #[test]
    fn concurrent_stores_do_not_lose_updates() {
        let file = temp_file();
        let handles: Vec<_> = (0..20)
            .map(|i| {
                let f = file.clone();
                std::thread::spawn(move || {
                    ChannelPresetStore::new(f)
                        .save(&preset(&format!("P{i}"), &[]))
                        .unwrap()
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        let saved = ChannelPresetStore::new(file.clone()).load_all().unwrap();
        assert_eq!(saved.len(), 20);
    }

    #[test]
    fn unsupported_import_version_does_not_change_existing() {
        let store = ChannelPresetStore::new(temp_file());
        let original = preset("O", &["a"]);
        store.save(&original).unwrap();
        let bad = temp_file();
        write_atomic(&bad, br#"{"version":2,"presets":[]}"#).unwrap();
        assert!(matches!(
            store.import_from_file(&bad),
            Err(PresetStoreError::UnsupportedVersion)
        ));
        assert_eq!(store.load_all().unwrap(), vec![original]);
    }

    #[test]
    fn legacy_file_without_kind_reads_as_chat() {
        let file = temp_file();
        let id = Uuid::new_v4();
        write_atomic(
            &file,
            format!(
                r#"{{"version":1,"presets":[{{"id":"{id}","name":"옛 묶음","channels":["a"]}}]}}"#
            )
            .as_bytes(),
        )
        .unwrap();
        let loaded = ChannelPresetStore::new(file).load_all().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "옛 묶음");
        assert_eq!(loaded[0].kind, PresetKind::Chat);
    }

    #[test]
    fn save_and_restore_keep_kind_and_position() {
        let store = ChannelPresetStore::new(temp_file());
        let live = Preset::new("L", vec!["x".into()], PresetKind::Live);
        store.save(&preset("first", &[])).unwrap();
        store.save(&live).unwrap();
        assert_eq!(store.load_all().unwrap()[1].kind, PresetKind::Live);
        store.delete(live.id).unwrap();
        store.restore(&live, 0).unwrap();
        let all = store.load_all().unwrap();
        assert_eq!(all[0].id, live.id);
        assert_eq!(all[0].kind, PresetKind::Live);
        assert_eq!(all.iter().filter(|p| p.kind == PresetKind::Chat).count(), 1);
    }
}
