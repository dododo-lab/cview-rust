//! 원본: `Tests/CViewPersistenceTests/UserSchemaV1Tests.swift`, `UserSchemaV1StoreOpenTests.swift`,
//! `UserMigrationPlanTests.swift`, `UnversionedEvolutionRuleTests.swift`, `HistoricalStoreUpgradeTests.swift`.
//!
//! CoreData 엔티티 버전 해시 골든(`user-schema-v1.hashes.json`)과 `.store` 픽스처는 SQLite 포팅에
//! 대응물이 없어 옮기지 않았다. 레거시 스토어는 `schema::migrate_to(1)` 로 합성한다.
//! 모양 골든(`tests/fixtures/user-schema-v1.shape.json`)은 Swift 쪽 파일을 그대로 쓴다.

use cview_persistence::schema::{
    self, AttributeShape, EntityShape, RelationshipShape, SchemaShape, StorePreflight,
    CURRENT_SCHEMA_VERSION, MIGRATIONS, USER_SCHEMA_V1_MODELS,
};
use cview_persistence::DataStore;
use rusqlite::Connection;
use std::collections::HashSet;
use std::path::Path;

fn golden_shape() -> SchemaShape {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/user-schema-v1.shape.json");
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

fn live_shape() -> SchemaShape {
    let s = DataStore::open_in_memory().unwrap();
    s.with_connection(SchemaShape::from_connection).unwrap()
}

// MARK: - UserSchemaV1

#[test]
fn v1_models_match_live_tables() {
    let live: HashSet<String> = live_shape().entities.into_iter().map(|e| e.name).collect();
    let v1: HashSet<String> = USER_SCHEMA_V1_MODELS
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(live, v1);
    assert_eq!(live.len(), 14);
}

#[test]
fn v1_shape_matches_golden() {
    assert_eq!(live_shape(), golden_shape());
}

#[test]
fn opens_in_memory() {
    let s = DataStore::open_in_memory().unwrap();
    assert_eq!(s.table_row_count("PersistedChannel").unwrap(), 0);
}

// MARK: - 마이그레이션 계획

#[test]
fn versions_strictly_increase_and_end_at_current() {
    let versions: Vec<i64> = MIGRATIONS.iter().map(|m| m.version).collect();
    assert!(!versions.is_empty());
    for pair in versions.windows(2) {
        assert!(pair[0] < pair[1]);
    }
    assert_eq!(*versions.last().unwrap(), CURRENT_SCHEMA_VERSION);
}

#[test]
fn migrate_is_idempotent_and_rejects_newer() {
    let mut conn = Connection::open_in_memory().unwrap();
    assert_eq!(schema::migrate(&mut conn).unwrap(), CURRENT_SCHEMA_VERSION);
    assert_eq!(schema::migrate(&mut conn).unwrap(), CURRENT_SCHEMA_VERSION);
    conn.pragma_update(None, "user_version", CURRENT_SCHEMA_VERSION + 1)
        .unwrap();
    assert!(matches!(
        schema::migrate(&mut conn),
        Err(cview_persistence::PersistenceError::NewerSchema { .. })
    ));
}

// MARK: - 스토어 열기

#[test]
fn v1_written_store_reopens() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fresh.store");
    {
        let s = DataStore::open(&path).unwrap();
        s.set_favorite(
            "t4",
            true,
            Some(&cview_core::models::ChannelInfo::new("t4", "T4")),
        )
        .unwrap();
    }
    let reopened = DataStore::open(&path).unwrap();
    let ch = reopened.fetch_channel("t4").unwrap().unwrap();
    assert!(ch.is_favorite);
    assert_eq!(StorePreflight::check(&path), StorePreflight::MatchesTarget);
}

#[test]
fn newer_store_is_unreadable_and_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("newer.store");
    {
        let mut conn = Connection::open(&path).unwrap();
        schema::migrate(&mut conn).unwrap();
        conn.pragma_update(None, "user_version", CURRENT_SCHEMA_VERSION + 1)
            .unwrap();
    }
    let (preflight, error) = StorePreflight::inspect(&path);
    assert_eq!(preflight, StorePreflight::Unreadable);
    assert!(error.is_some());
    assert!(DataStore::open(&path).is_err());
}

// MARK: - 과거 스토어 업그레이드

#[test]
fn legacy_nine_model_store_upgrades_without_losing_data() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("upgrade.store");
    {
        let mut conn = Connection::open(&path).unwrap();
        assert_eq!(schema::migrate_to(&mut conn, 1).unwrap(), 1);
        let tables: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'table'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(tables, 9, "레거시 스토어는 9개 모델");
        conn.execute_batch(
            "INSERT INTO PersistedChannel (channelId, channelName, isFavorite, addedAt, memo)
                 VALUES ('fixture-favorite', '이전 버전 즐겨찾기', 1, 0, 'fixture memo');
             INSERT INTO PersistedStatistic (channelId, timestamp, viewerCount, watchDuration, channel)
                 VALUES ('fixture-favorite', 0, 42, 0, 1);
             INSERT INTO WatchHistory (channelId, channelName, startedAt, endedAt, duration)
                 VALUES ('fixture-favorite', '이전 버전 즐겨찾기', 0, 120000000000, 120);
             INSERT INTO PersistedSetting (key, valueData, updatedAt)
                 VALUES ('fixture.volume', CAST('0.37' AS BLOB), 0);
             INSERT INTO CachedLiveChannel (channelId, channelName, liveTitle, viewerCount, isLive, sources, fetchedAt)
                 VALUES ('live', '라이브', '', 17, 1, 'live-list', 0);",
        )
        .unwrap();
    }
    assert_eq!(
        StorePreflight::check(&path),
        StorePreflight::NeedsLightweightUpgrade
    );
    let original = std::fs::read(&path).unwrap();
    let copy = dir.path().join("copy.store");
    std::fs::write(&copy, &original).unwrap();

    let s = DataStore::open(&copy).unwrap();
    let ch = s.fetch_channel("fixture-favorite").unwrap().unwrap();
    assert!(ch.is_favorite);
    assert_eq!(ch.channel_name, "이전 버전 즐겨찾기");
    assert_eq!(ch.memo.as_deref(), Some("fixture memo"));
    let viewer: i64 = s
        .with_connection(|c| {
            Ok(c.query_row(
                "SELECT viewerCount FROM PersistedStatistic WHERE channel = ?",
                [ch.id],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(viewer, 42);
    let history = s.all_watch_history_rows().unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].duration, 120.0);
    assert!(history[0].ended_at.is_some());
    assert_eq!(s.load_setting::<f64>("fixture.volume").unwrap(), Some(0.37));
    assert_eq!(
        s.fetch_all_cached_live_channels().unwrap()[0].viewer_count,
        17
    );

    // 새 모델에 쓸 수 있다.
    s.record_vod_playback(123, 37.0, 100.0, "new model", "fixture", None)
        .unwrap();
    drop(s);
    let reopened = DataStore::open(&copy).unwrap();
    let playback = reopened.fetch_vod_playback(123).unwrap().unwrap();
    assert_eq!(playback.last_position_sec, 37.0);
    assert_eq!(reopened.all_watch_history_rows().unwrap().len(), 1);
    assert_eq!(
        std::fs::read(&path).unwrap(),
        original,
        "원본 사본이 바뀌었다"
    );
}

// MARK: - 버전 없는 진화 규칙

fn attr(name: &str, kind: u32, optional: bool, has_default: bool) -> AttributeShape {
    AttributeShape {
        name: name.into(),
        kind,
        optional,
        has_default,
    }
}

fn entity(name: &str, attributes: Vec<AttributeShape>) -> EntityShape {
    EntityShape {
        name: name.into(),
        attributes,
        relationships: vec![],
    }
}

fn shape(attributes: Vec<AttributeShape>, extra: Vec<EntityShape>) -> SchemaShape {
    let mut entities = vec![entity("A", attributes)];
    entities.extend(extra);
    SchemaShape { entities }
}

#[test]
fn live_schema_obeys_rule() {
    assert!(SchemaShape::evolution_violations(&golden_shape(), &live_shape()).is_empty());
}

#[test]
fn allowed_changes() {
    let base = shape(
        vec![attr("x", 700, false, false), attr("y", 300, false, false)],
        vec![entity("Gone", vec![])],
    );
    let changed = shape(
        vec![
            attr("x", 700, false, false),
            attr("opt", 700, true, false),
            attr("def", 300, false, true),
        ],
        vec![entity("Added", vec![])],
    );
    assert!(SchemaShape::evolution_violations(&base, &changed).is_empty());
}

#[test]
fn forbids_type_change() {
    let base = shape(vec![attr("x", 700, false, false)], vec![]);
    let changed = shape(vec![attr("x", 300, false, false)], vec![]);
    assert_eq!(SchemaShape::evolution_violations(&base, &changed).len(), 1);
}

#[test]
fn forbids_optional_to_required() {
    let base = shape(vec![attr("x", 700, true, false)], vec![]);
    let changed = shape(vec![attr("x", 700, false, false)], vec![]);
    assert_eq!(SchemaShape::evolution_violations(&base, &changed).len(), 1);
}

#[test]
fn forbids_required_without_default() {
    let base = shape(vec![attr("x", 700, false, false)], vec![]);
    let changed = shape(
        vec![attr("x", 700, false, false), attr("z", 300, false, false)],
        vec![],
    );
    assert_eq!(SchemaShape::evolution_violations(&base, &changed).len(), 1);
}

#[test]
fn forbids_relationship_change() {
    let relation = RelationshipShape {
        name: "r".into(),
        destination: "B".into(),
        to_many: true,
        optional: true,
        delete_rule: 2,
    };
    let mut moved = relation.clone();
    moved.destination = "C".into();
    let base = SchemaShape {
        entities: vec![EntityShape {
            name: "A".into(),
            attributes: vec![],
            relationships: vec![relation],
        }],
    };
    let changed = SchemaShape {
        entities: vec![EntityShape {
            name: "A".into(),
            attributes: vec![],
            relationships: vec![moved],
        }],
    };
    assert_eq!(SchemaShape::evolution_violations(&base, &changed).len(), 1);
}

#[test]
fn synthetic_table_shapes_follow_rule() {
    let before = Connection::open_in_memory().unwrap();
    before
        .execute_batch(
            "CREATE TABLE SyntheticBefore (id INTEGER PRIMARY KEY, title TEXT NOT NULL);",
        )
        .unwrap();
    let after = Connection::open_in_memory().unwrap();
    after
        .execute_batch(
            "CREATE TABLE SyntheticAfter (id INTEGER PRIMARY KEY, title TEXT NOT NULL, note TEXT);",
        )
        .unwrap();
    let b = SchemaShape::from_connection(&before).unwrap();
    let a = SchemaShape::from_connection(&after).unwrap();
    assert_ne!(
        b.entities.iter().map(|e| &e.name).collect::<Vec<_>>(),
        a.entities.iter().map(|e| &e.name).collect::<Vec<_>>()
    );
    assert!(SchemaShape::evolution_violations(&b, &a).is_empty());
    assert_eq!(
        b.entities[0]
            .attributes
            .iter()
            .map(|x| x.name.as_str())
            .collect::<Vec<_>>(),
        vec!["title"]
    );
}
