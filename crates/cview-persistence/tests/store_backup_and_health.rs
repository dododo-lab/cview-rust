//! 원본: `Tests/CViewPersistenceTests/StoreBackupTests.swift`, `StoreOpenHealthTests.swift`.
//!
//! 옛 CoreData 픽스처(`legacy-20260510.store`) 대신 `schema::migrate_to(1)` 로 만든 v1 스토어를
//! "경량 업그레이드 필요" 스토어로 쓴다. `store_open` 캡처 줄(E7) 테스트는 계측을 포팅하지 않아 뺐다.

use cview_persistence::schema::{self, StorePreflight};
use cview_persistence::store_backup::{BackupEntry, BackupFailure, BackupReason};
use cview_persistence::time::{adding, from_unix, Date};
use cview_persistence::{DataStore, StoreBackup, StoreOpenHealthRecorder};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

type CopyFn = Arc<dyn Fn(&Path, &Path) -> std::io::Result<()> + Send + Sync>;
type ClockFn = Arc<dyn Fn() -> Date + Send + Sync>;

// MARK: - 백업

struct Sandbox {
    _dir: tempfile::TempDir,
    root: PathBuf,
    store: PathBuf,
}

impl Sandbox {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        Self {
            store: root.join("CView_v2.store"),
            root,
            _dir: dir,
        }
    }

    fn backups(&self) -> PathBuf {
        self.root.join("Backups")
    }

    fn write(&self, name: &str, text: &str) {
        let p = self.root.join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.root.join(name)).unwrap()
    }

    fn write_full_store(&self, tag: &str) {
        self.write("CView_v2.store", &format!("store-{tag}"));
        self.write("CView_v2.store-wal", &format!("wal-{tag}"));
        self.write("CView_v2.store-shm", &format!("shm-{tag}"));
        self.write(
            ".CView_v2_SUPPORT/_EXTERNAL_DATA/blob",
            &format!("support-{tag}"),
        );
    }

    fn backup(&self) -> StoreBackup {
        self.backup_with(None, None, None)
    }

    fn backup_with(
        &self,
        capacity: Option<i64>,
        copy: Option<CopyFn>,
        clock: Option<ClockFn>,
    ) -> StoreBackup {
        StoreBackup::with_hooks(
            &self.store,
            None,
            StoreBackup::DEFAULT_KEEP,
            Arc::new(move |_| capacity),
            copy.unwrap_or_else(|| Arc::new(cview_persistence::store_backup::copy_item)),
            clock.unwrap_or_else(|| Arc::new(cview_persistence::time::now)),
        )
    }

    fn folder_names(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(self.backups())
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }
}

/// 호출마다 1분씩 흐르는 시계.
fn ticking_clock() -> ClockFn {
    let current = Arc::new(Mutex::new(from_unix(1_790_000_000.0)));
    Arc::new(move || {
        let mut c = current.lock().unwrap();
        *c = adding(&c, 60.0);
        *c
    })
}

fn failing_copy(suffix: &'static str) -> CopyFn {
    Arc::new(move |from: &Path, to: &Path| {
        if from.to_string_lossy().ends_with(suffix) {
            return Err(std::io::Error::other("injected copy failure"));
        }
        cview_persistence::store_backup::copy_item(from, to)
    })
}

#[test]
fn copies_store_files_and_leaves_originals() {
    let b = Sandbox::new();
    b.write_full_store("A");
    let entry = b
        .backup()
        .snapshot(
            BackupReason::BeforeUpgrade,
            "812",
            Some("needsLightweightUpgrade"),
        )
        .unwrap();
    assert_eq!(
        entry.files,
        vec![
            "CView_v2.store",
            "CView_v2.store-wal",
            "CView_v2.store-shm",
            ".CView_v2_SUPPORT"
        ]
    );
    assert_eq!(entry.reason, BackupReason::BeforeUpgrade);
    assert_eq!(entry.app_build, "812");
    assert_eq!(entry.preflight.as_deref(), Some("needsLightweightUpgrade"));
    assert!(entry.id.starts_with("CView_v2-") && entry.id.ends_with("-b812"));
    let expected: usize = ["store-A", "wal-A", "shm-A", "support-A"]
        .iter()
        .map(|s| s.len())
        .sum();
    assert_eq!(entry.bytes, expected as i64);
    let folder = format!("Backups/{}/", entry.id);
    assert_eq!(b.read(&format!("{folder}CView_v2.store")), "store-A");
    assert_eq!(b.read(&format!("{folder}CView_v2.store-wal")), "wal-A");
    assert_eq!(b.read(&format!("{folder}CView_v2.store-shm")), "shm-A");
    assert_eq!(
        b.read(&format!("{folder}.CView_v2_SUPPORT/_EXTERNAL_DATA/blob")),
        "support-A"
    );
    assert_eq!(b.read("CView_v2.store"), "store-A");
    assert_eq!(b.read("CView_v2.store-wal"), "wal-A");
    let listed = b.backup().list();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, entry.id);
    assert_eq!(listed[0].files, entry.files);
    assert_eq!(listed[0].bytes, entry.bytes);
}

#[test]
fn succeeds_without_wal_and_shm() {
    let b = Sandbox::new();
    b.write("CView_v2.store", "only");
    let entry = b
        .backup()
        .snapshot(BackupReason::Manual, "1", None)
        .unwrap();
    assert_eq!(entry.files, vec!["CView_v2.store"]);
    assert_eq!(
        b.read(&format!("Backups/{}/CView_v2.store", entry.id)),
        "only"
    );
}

#[test]
fn missing_store_returns_none_without_backup_folder() {
    let b = Sandbox::new();
    assert!(b
        .backup()
        .snapshot(BackupReason::BeforeUpgrade, "1", None)
        .is_none());
    assert!(!b.backups().exists());
    assert!(b.backup().list().is_empty());
}

#[test]
fn low_space_skips() {
    let b = Sandbox::new();
    b.write_full_store("A");
    let total = StoreBackup::store_bytes(&b.store);
    assert!(b
        .backup_with(Some(total * 2 - 1), None, None)
        .snapshot(BackupReason::BeforeUpgrade, "1", None)
        .is_none());
    assert!(b.backup().list().is_empty());
    assert!(b
        .folder_names()
        .iter()
        .all(|n| !n.starts_with(".incomplete-")));
    assert!(b
        .backup_with(Some(total * 2), None, None)
        .snapshot(BackupReason::BeforeUpgrade, "1", None)
        .is_some());
}

#[test]
fn interrupted_copy_leaves_no_visible_backup() {
    let b = Sandbox::new();
    b.write_full_store("A");
    let failing = b.backup_with(None, Some(failing_copy("-wal")), None);
    assert!(failing
        .snapshot(BackupReason::BeforeUpgrade, "1", None)
        .is_none());
    assert!(b.backup().list().is_empty());
    b.write(
        &format!(
            "Backups/.incomplete-{}/CView_v2.store",
            uuid::Uuid::new_v4()
        ),
        "partial",
    );
    assert!(b.backup().list().is_empty());
    let entry = b
        .backup()
        .snapshot(BackupReason::BeforeUpgrade, "1", None)
        .unwrap();
    assert_eq!(b.folder_names(), vec![entry.id]);
}

#[test]
fn keeps_two_newest() {
    let b = Sandbox::new();
    b.write_full_store("A");
    let backup = b.backup_with(None, None, Some(ticking_clock()));
    let ids: Vec<String> = (0..3)
        .map(|_| {
            backup
                .snapshot(BackupReason::BeforeUpgrade, "7", None)
                .unwrap()
                .id
        })
        .collect();
    assert_eq!(
        ids.iter().collect::<std::collections::HashSet<_>>().len(),
        3
    );
    let listed: Vec<_> = backup.list().into_iter().map(|e| e.id).collect();
    assert_eq!(listed, vec![ids[2].clone(), ids[1].clone()]);
    assert!(!b.folder_names().contains(&ids[0]));
}

#[test]
fn manifest_has_no_absolute_path() {
    let b = Sandbox::new();
    b.write_full_store("A");
    let entry = b
        .backup()
        .snapshot(BackupReason::BeforeUpgrade, "3", None)
        .unwrap();
    let manifest = b.read(&format!("Backups/{}/manifest.json", entry.id));
    assert!(
        !manifest.contains("/tmp/") && !manifest.contains("/var/") && !manifest.contains("/home/")
    );
    assert!(!manifest.contains(&*b.root.to_string_lossy()));
    assert!(!manifest.contains("\\/"));
    assert!(manifest.contains("\"v\" : 1") || manifest.contains("\"v\": 1"));
    assert!(manifest.contains("CView_v2.store-wal"));
}

#[test]
fn remove_all_clears_app_made_folders() {
    let b = Sandbox::new();
    b.write_full_store("A");
    let backup = b.backup_with(None, None, Some(ticking_clock()));
    let first = backup
        .snapshot(BackupReason::BeforeUpgrade, "1", None)
        .unwrap();
    backup
        .snapshot(BackupReason::BeforeUpgrade, "1", None)
        .unwrap();
    backup.restore(&first).unwrap();
    b.write("Backups/.incomplete-leftover/CView_v2.store", "partial");
    b.write("Backups/notes.txt", "사용자 파일");
    assert_eq!(b.folder_names().len(), 5);

    backup.remove_all().unwrap();
    assert!(backup.list().is_empty());
    assert_eq!(b.folder_names(), vec!["notes.txt"]);

    std::fs::remove_file(b.backups().join("notes.txt")).unwrap();
    backup.snapshot(BackupReason::Manual, "1", None).unwrap();
    backup.remove_all().unwrap();
    assert!(!b.backups().exists());
}

#[test]
fn restore_moves_current_aside() {
    let b = Sandbox::new();
    b.write_full_store("A");
    let backup = b.backup_with(None, None, Some(ticking_clock()));
    let entry = backup
        .snapshot(BackupReason::BeforeUpgrade, "1", None)
        .unwrap();
    b.write_full_store("B");

    backup.restore(&entry).unwrap();
    assert_eq!(b.read("CView_v2.store"), "store-A");
    assert_eq!(b.read("CView_v2.store-wal"), "wal-A");
    assert_eq!(b.read(".CView_v2_SUPPORT/_EXTERNAL_DATA/blob"), "support-A");
    let replaced: Vec<_> = b
        .folder_names()
        .into_iter()
        .filter(|n| n.starts_with("replaced-"))
        .collect();
    assert_eq!(replaced.len(), 1);
    assert_eq!(
        b.read(&format!("Backups/{}/CView_v2.store", replaced[0])),
        "store-B"
    );
    let listed: Vec<_> = backup.list().into_iter().map(|e| e.id).collect();
    assert_eq!(listed, vec![entry.id.clone()]);

    backup.restore(&entry).unwrap();
    let again: Vec<_> = b
        .folder_names()
        .into_iter()
        .filter(|n| n.starts_with("replaced-"))
        .collect();
    assert_eq!(again.len(), 1);
    assert_ne!(again, replaced);
    assert_eq!(
        b.read(&format!("Backups/{}/CView_v2.store", again[0])),
        "store-A"
    );
}

#[test]
fn restore_rolls_back_on_copy_failure() {
    let b = Sandbox::new();
    b.write_full_store("A");
    let entry = b
        .backup()
        .snapshot(BackupReason::BeforeUpgrade, "1", None)
        .unwrap();
    b.write_full_store("B");
    let failing = b.backup_with(None, Some(failing_copy("-shm")), None);
    assert!(matches!(failing.restore(&entry), Err(BackupFailure::Io(_))));
    assert_eq!(b.read("CView_v2.store"), "store-B");
    assert_eq!(b.read("CView_v2.store-wal"), "wal-B");
    assert_eq!(b.read("CView_v2.store-shm"), "shm-B");
    assert_eq!(b.read(".CView_v2_SUPPORT/_EXTERNAL_DATA/blob"), "support-B");
    assert!(b.folder_names().iter().all(|n| !n.starts_with("replaced-")));
    assert_eq!(
        b.read(&format!("Backups/{}/CView_v2.store", entry.id)),
        "store-A"
    );
}

#[test]
fn restore_rejects_unknown_entry() {
    let b = Sandbox::new();
    b.write_full_store("A");
    let backup = b.backup();
    let entry = backup
        .snapshot(BackupReason::BeforeUpgrade, "1", None)
        .unwrap();
    std::fs::remove_file(b.backups().join(&entry.id).join("CView_v2.store")).unwrap();
    assert!(matches!(
        backup.restore(&entry),
        Err(BackupFailure::StoreFileMissing)
    ));
    backup.remove_all().unwrap();
    assert!(matches!(
        backup.restore(&entry),
        Err(BackupFailure::EntryNotFound)
    ));
    let escaping = BackupEntry {
        id: "CView_v2-/../../escape".into(),
        created_at: cview_persistence::time::now(),
        app_build: "1".into(),
        reason: BackupReason::Manual,
        preflight: None,
        bytes: 0,
        files: vec!["CView_v2.store".into()],
    };
    assert!(matches!(
        backup.restore(&escaping),
        Err(BackupFailure::EntryNotFound)
    ));
    assert_eq!(b.read("CView_v2.store"), "store-A");
}

// MARK: - 열기 기록

struct OpenBox {
    _dir: tempfile::TempDir,
    root: PathBuf,
}

impl OpenBox {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        Self {
            root: dir.path().to_path_buf(),
            _dir: dir,
        }
    }

    fn store_path(&self, folder: &str) -> PathBuf {
        let d = self.root.join(folder);
        std::fs::create_dir_all(&d).unwrap();
        d.join("CView_v2.store")
    }

    fn recorder(&self) -> StoreOpenHealthRecorder {
        StoreOpenHealthRecorder::new(self.root.join("logs/store-health.json"))
    }

    fn open(
        &self,
        path: &Path,
        health: &StoreOpenHealthRecorder,
    ) -> cview_persistence::Result<DataStore> {
        DataStore::create_container_at(path, health, &StoreBackup::new(path), "900")
    }
}

/// v1(레거시 9 모델) 스토어 파일을 만든다 — Swift 의 CoreData 픽스처 자리.
fn write_legacy_store(path: &Path) {
    let mut conn = rusqlite::Connection::open(path).unwrap();
    schema::migrate_to(&mut conn, 1).unwrap();
    conn.execute(
        "INSERT INTO PersistedChannel (channelId, channelName, isFavorite, addedAt) VALUES ('ch', '채널', 1, 0)",
        [],
    )
    .unwrap();
}

#[test]
fn garbage_store_fails_then_recovers() {
    let b = OpenBox::new();
    let health = b.recorder();
    let garbage_path = b.store_path("garbage");
    let garbage: Vec<u8> = (0..8_192u32)
        .map(|i| (i.wrapping_mul(31).wrapping_add(7)) as u8)
        .collect();
    std::fs::write(&garbage_path, &garbage).unwrap();

    assert!(b.open(&garbage_path, &health).is_err());
    let failed = health.load();
    assert_eq!(
        (
            failed.total_opens,
            failed.failures,
            failed.consecutive_failures
        ),
        (1, 1, 1)
    );
    assert_eq!(
        failed.last_preflight.as_deref(),
        Some(StorePreflight::Unreadable.raw_value())
    );
    assert!(failed.last_failure_at.is_some() && failed.last_success_at.is_none());
    let error = failed.last_error.clone().unwrap();
    assert!(!error.domain.is_empty());
    assert_eq!(std::fs::read(&garbage_path).unwrap(), garbage);
    assert!(StoreBackup::new(&garbage_path).list().is_empty());

    b.open(&b.store_path("fresh"), &health).unwrap();
    let recovered = health.load();
    assert_eq!(
        (
            recovered.consecutive_failures,
            recovered.failures,
            recovered.total_opens
        ),
        (0, 1, 2)
    );
    assert!(recovered.last_success_at.is_some() && recovered.last_open_ms.is_some());
    assert_eq!(
        recovered.last_preflight.as_deref(),
        Some(StorePreflight::Missing.raw_value())
    );
    assert_eq!(recovered.last_error, failed.last_error);
    assert_eq!(recovered.app_build.as_deref(), Some("900"));

    let text = std::fs::read_to_string(health.file_path()).unwrap();
    for fragment in [
        &*b.root.to_string_lossy(),
        "/tmp/",
        "/home/",
        "CView_v2.store",
    ] {
        assert!(!text.contains(fragment), "경로 조각 '{fragment}'");
    }
    let reloaded = StoreOpenHealthRecorder::new(health.file_path()).load();
    assert_eq!(reloaded.total_opens, recovered.total_opens);
    assert_eq!(reloaded.failures, recovered.failures);
    assert_eq!(
        reloaded.consecutive_failures,
        recovered.consecutive_failures
    );
    assert_eq!(reloaded.last_error, recovered.last_error);
    // JSON f64 왕복은 1 ULP 차이가 날 수 있다.
    assert!((reloaded.last_open_ms.unwrap() - recovered.last_open_ms.unwrap()).abs() < 1e-6);
    assert_eq!(reloaded.last_preflight, recovered.last_preflight);
    assert_eq!(reloaded.store_bytes, recovered.store_bytes);
    let a = reloaded.last_success_at.unwrap();
    let e = recovered.last_success_at.unwrap();
    assert!(cview_persistence::time::interval_since(&a, &e).abs() < 1.0);
}

#[test]
fn consecutive_failures_accumulate_across_recorders() {
    let b = OpenBox::new();
    let path = b.store_path("garbage");
    std::fs::write(&path, vec![0x5A; 4_096]).unwrap();
    for attempt in 1..=3 {
        let health = b.recorder();
        assert!(b.open(&path, &health).is_err());
        assert_eq!(health.load().consecutive_failures, attempt);
    }
}

#[test]
fn preflight_classifies_stores() {
    let b = OpenBox::new();
    let path = b.store_path("current");
    assert_eq!(StorePreflight::check(&path), StorePreflight::Missing);
    drop(b.open(&path, &b.recorder()).unwrap());
    assert_eq!(StorePreflight::check(&path), StorePreflight::MatchesTarget);

    let legacy = b.store_path("legacy");
    write_legacy_store(&legacy);
    assert_eq!(
        StorePreflight::check(&legacy),
        StorePreflight::NeedsLightweightUpgrade
    );

    let garbage = b.store_path("garbage");
    std::fs::write(&garbage, vec![0x41; 4_096]).unwrap();
    assert_eq!(StorePreflight::check(&garbage), StorePreflight::Unreadable);
}

#[test]
fn upgrade_backs_up_once_and_normal_launch_does_not() {
    let b = OpenBox::new();
    let health = b.recorder();
    let path = b.store_path("upgrade");
    write_legacy_store(&path);
    let fixture = std::fs::read(&path).unwrap();

    let store = b.open(&path, &health).unwrap();
    assert!(
        store.fetch_channel("ch").unwrap().unwrap().is_favorite,
        "마이그레이션이 행을 잃었다"
    );
    drop(store);
    let backups = StoreBackup::new(&path).list();
    assert_eq!(backups.len(), 1);
    let entry = &backups[0];
    assert_eq!(entry.reason, BackupReason::BeforeUpgrade);
    assert_eq!(
        entry.preflight.as_deref(),
        Some(StorePreflight::NeedsLightweightUpgrade.raw_value())
    );
    assert_eq!(entry.app_build, "900");
    let backed_up = path
        .parent()
        .unwrap()
        .join(format!("Backups/{}/CView_v2.store", entry.id));
    assert_eq!(std::fs::read(backed_up).unwrap(), fixture);
    let upgraded = health.load();
    assert_eq!(
        upgraded.last_backup_folder.as_deref(),
        Some(entry.id.as_str())
    );
    assert!(upgraded.store_bytes.unwrap_or(0) >= fixture.len() as i64);

    drop(b.open(&path, &health).unwrap());
    let ids: Vec<_> = StoreBackup::new(&path)
        .list()
        .into_iter()
        .map(|e| e.id)
        .collect();
    assert_eq!(ids, vec![entry.id.clone()]);
    let normal = health.load();
    assert_eq!(
        normal.last_preflight.as_deref(),
        Some(StorePreflight::MatchesTarget.raw_value())
    );
    assert_eq!(
        normal.last_backup_folder.as_deref(),
        Some(entry.id.as_str())
    );
    assert_eq!((normal.consecutive_failures, normal.total_opens), (0, 2));
}
