//! 원본: `Tests/CViewPersistenceTests/StoreMaintenanceTests.swift`, `UserRecordFileTests.swift`,
//! `Swift6MutationRollbackTests.swift`.
//!
//! SwiftData 변경 이력은 이 포팅에서 메모리 커밋 기록(`history_transaction_count`)이다.
//! 롤백 테스트는 `inject_commit_failure` 로 다음 커밋 한 번을 실패시킨다.

mod common;

use common::*;
use cview_persistence::store::maintenance::StoreMaintenancePolicy;
use cview_persistence::user_record_file::ReadOutcome;
use cview_persistence::{DataStore, UserRecordFile};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

// MARK: - 유지보수

fn file_store() -> (tempfile::TempDir, DataStore) {
    let dir = tempfile::tempdir().unwrap();
    let s = DataStore::open(dir.path().join("maint.store")).unwrap();
    (dir, s)
}

#[test]
fn same_setting_value_does_not_write_again() {
    let (_d, s) = file_store();
    s.save_setting("volume", &0.5).unwrap();
    let after_first = s.history_transaction_count();
    assert!(after_first > 0);
    s.save_setting("volume", &0.5).unwrap();
    s.save_settings(&HashMap::from([(
        "volume".to_string(),
        serde_json::to_vec(&0.5).unwrap(),
    )]))
    .unwrap();
    assert_eq!(s.history_transaction_count(), after_first);
    s.save_settings(&HashMap::from([(
        "volume".to_string(),
        serde_json::to_vec(&0.8).unwrap(),
    )]))
    .unwrap();
    assert_eq!(s.history_transaction_count(), after_first + 1);
    assert_eq!(s.load_setting::<f64>("volume").unwrap(), Some(0.8));
}

#[tokio::test]
async fn purge_removes_history_and_keeps_data() {
    let (_d, s) = file_store();
    for i in 0..12 {
        s.save_setting(&format!("k{i}"), &i).unwrap();
    }
    assert!(s.history_transaction_count() >= 12);
    // 이력은 cutoff "이전" 만 지운다. 마지막 커밋과 같은 시각(macOS 시계 해상도)이면 남으므로
    // cutoff 를 마지막 커밋보다 확실히 뒤로 둔다.
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    let windows = s.purge_change_history(now(), 1.0).await;
    assert!(windows.is_some());
    assert_eq!(s.history_transaction_count(), 0);
    for i in 0..12 {
        assert_eq!(s.load_setting::<i64>(&format!("k{i}")).unwrap(), Some(i));
    }
    let again = s
        .purge_change_history(now(), StoreMaintenancePolicy::HISTORY_DELETE_WINDOW)
        .await;
    assert_eq!(again, Some(0));
}

// MARK: - 사용자 기록 파일

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Record {
    v: i64,
    name: String,
    when: i64,
}

fn rec(name: &str, when: i64) -> Record {
    Record {
        v: 1,
        name: name.into(),
        when,
    }
}

#[test]
fn record_round_trip_is_compact_and_sorted() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested/record.json");
    let value = rec("가", 1_700_000_000);
    let bytes = UserRecordFile::write(&value, &path, None).unwrap();
    assert_eq!(
        String::from_utf8(bytes.clone()).unwrap(),
        r#"{"name":"가","v":1,"when":1700000000}"#
    );
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert_eq!(UserRecordFile::read::<Record>(&path, 1), Some(value));
}

#[test]
fn record_same_bytes_skip_write() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("record.json");
    let value = rec("a", 1);
    let first = UserRecordFile::write(&value, &path, None).unwrap();
    std::fs::write(&path, b"sentinel").unwrap();
    assert_eq!(
        UserRecordFile::write(&value, &path, Some(&first)),
        Some(first.clone())
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"sentinel");
    let second = UserRecordFile::write(&rec("b", 1), &path, Some(&first)).unwrap();
    assert_ne!(second, first);
    assert_eq!(std::fs::read(&path).unwrap(), second);
}

#[test]
fn record_corrupt_inputs() {
    for text in [
        "not json",
        r#"{"name":"a"}"#,
        r#"{"v":"1","name":"a","when":0}"#,
        r#"{"v":1,"name":3}"#,
    ] {
        assert!(
            matches!(
                UserRecordFile::decode::<Record>(text.as_bytes(), 1),
                ReadOutcome::Corrupt
            ),
            "손상으로 분류되지 않았다: {text}"
        );
    }
}

#[test]
fn record_version_gate() {
    match UserRecordFile::decode::<Record>(br#"{"v":2,"name":"a","when":0}"#, 1) {
        ReadOutcome::Newer { version } => assert_eq!(version, 2),
        _ => panic!("상위 버전을 읽었다"),
    }
    match UserRecordFile::decode::<Record>(br#"{"v":0,"name":"a","when":0}"#, 1) {
        ReadOutcome::Loaded(value, _) => assert_eq!(value.name, "a"),
        _ => panic!("하위 버전을 버렸다"),
    }
}

#[test]
fn record_missing_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("absent.json");
    assert!(matches!(
        UserRecordFile::load::<Record>(&path, 1),
        ReadOutcome::Missing
    ));
    assert_eq!(UserRecordFile::read::<Record>(&path, 1), None);
}

#[cfg(unix)]
#[test]
fn record_replacement_is_atomic() {
    use std::os::unix::fs::MetadataExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("record.json");
    UserRecordFile::write(&rec("a", 1), &path, None).unwrap();
    let before = std::fs::metadata(&path).unwrap().ino();
    UserRecordFile::write(&rec("b", 1), &path, None).unwrap();
    let after = std::fs::metadata(&path).unwrap().ino();
    assert_ne!(before, after, "같은 파일을 제자리에서 덮어썼다");
    let names: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec!["record.json"]);
    assert_eq!(
        UserRecordFile::read::<Record>(&path, 1).map(|r| r.name),
        Some("b".to_string())
    );
}

// MARK: - 실패한 변경의 롤백

#[test]
fn failed_deletion_cannot_be_committed_by_later_write() {
    let s = store();
    s.add_vod_watch_later(1, "kept", "test", None, 0.0).unwrap();
    s.inject_commit_failure();
    assert!(s.remove_vod_watch_later(1).is_err());
    s.add_vod_watch_later(2, "later", "test", None, 0.0)
        .unwrap();
    let ids: HashSet<_> = s.fetch_vod_watch_later_ids().unwrap().into_iter().collect();
    assert_eq!(ids, HashSet::from([1, 2]));
}

#[test]
fn failed_insertion_cannot_leak_into_later_write() {
    let s = store();
    s.inject_commit_failure();
    assert!(s
        .add_vod_watch_later(1, "pending", "test", None, 0.0)
        .is_err());
    s.add_vod_watch_later(2, "later", "test", None, 0.0)
        .unwrap();
    assert_eq!(s.fetch_vod_watch_later_ids().unwrap(), vec![2]);
}
