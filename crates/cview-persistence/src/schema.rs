//! 스키마 정의와 `PRAGMA user_version` 기반 버전 마이그레이션.
//!
//! 원본: `UserSchemaV1.swift`(14개 @Model 고정 목록), `UserMigrationPlan.swift`(빈 계획),
//! `DataStore.swift` 의 `DataStore.schema`, 각 `Models/*.swift` 의 @Model 속성.
//!
//! SwiftData @Model 은 SQL 테이블로 옮겼다. 테이블 이름 = 모델 이름, 열 이름 = 속성 이름(camelCase),
//! 그리고 `id INTEGER PRIMARY KEY`(SwiftData 의 PersistentIdentifier 자리). 선언 타입은 CoreData
//! 속성 타입 코드와 1:1 로 대응한다 — TEXT(700) · INTEGER(300) · REAL(500) · BOOLEAN(800) ·
//! TIMESTAMP(900, epoch 나노초 정수) · BLOB(1000). `@Attribute(.unique)` 는 UNIQUE 제약이다.
//!
//! 버전 이력:
//! - 1: 2026-05-10 레거시 스토어(9개 모델 — `Fixtures/legacy-20260510.json`).
//! - 2: `UserSchemaV1`(14개 모델). CachedVOD · CategorySnapshot · PersistedClipPlayback ·
//!   PersistedVODPlayback · PersistedVODWatchLater 추가(전부 엔티티 추가 = 경량 마이그레이션).
//!
//! 이 빌드보다 새 버전의 스토어는 열지 않는다(다운그레이드 보호).

use crate::error::{PersistenceError, Result};
use rusqlite::{Connection, OpenFlags};
use std::path::Path;

/// 이 빌드가 만드는 최신 스키마 버전(`PRAGMA user_version`).
pub const CURRENT_SCHEMA_VERSION: i64 = 2;

/// `UserSchemaV1.models` — 14개 모델 이름(원본 순서).
pub const USER_SCHEMA_V1_MODELS: [&str; 14] = [
    "PersistedChannel",
    "PersistedStatistic",
    "PersistedSetting",
    "WatchHistory",
    "CachedImageEntry",
    "CachedLiveChannel",
    "CachedClip",
    "CachedVOD",
    "CachedCategoryStat",
    "CategorySnapshot",
    "PersistedClipPlayback",
    "PersistedVODPlayback",
    "PersistedVODWatchLater",
    "StreamBookmark",
];

/// `UserSchemaV1.versionIdentifier` — 1.0.0.
pub const USER_SCHEMA_V1_VERSION: (u32, u32, u32) = (1, 0, 0);

/// 마이그레이션 한 단계.
pub struct Migration {
    /// 이 단계를 적용한 뒤의 `user_version`.
    pub version: i64,
    /// 설명.
    pub description: &'static str,
    /// 적용할 SQL.
    pub sql: &'static str,
}

/// 2026-05-10 레거시 스토어 — 9개 모델.
const V1_LEGACY_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS PersistedChannel (
    id INTEGER PRIMARY KEY,
    channelId TEXT NOT NULL UNIQUE,
    channelName TEXT NOT NULL,
    imageURL TEXT,
    isFavorite BOOLEAN NOT NULL,
    lastWatched TIMESTAMP,
    addedAt TIMESTAMP NOT NULL,
    memo TEXT
);
CREATE TABLE IF NOT EXISTS PersistedStatistic (
    id INTEGER PRIMARY KEY,
    channelId TEXT NOT NULL,
    timestamp TIMESTAMP NOT NULL,
    viewerCount INTEGER NOT NULL,
    watchDuration REAL NOT NULL,
    averageLatency REAL,
    channel INTEGER REFERENCES PersistedChannel(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS PersistedStatistic_channel ON PersistedStatistic(channel);
CREATE TABLE IF NOT EXISTS PersistedSetting (
    id INTEGER PRIMARY KEY,
    key TEXT NOT NULL UNIQUE,
    valueData BLOB NOT NULL,
    updatedAt TIMESTAMP NOT NULL
);
CREATE TABLE IF NOT EXISTS WatchHistory (
    id INTEGER PRIMARY KEY,
    channelId TEXT NOT NULL,
    channelName TEXT NOT NULL,
    thumbnailURL TEXT,
    categoryName TEXT,
    startedAt TIMESTAMP NOT NULL,
    endedAt TIMESTAMP,
    duration REAL NOT NULL,
    updatedAt TIMESTAMP,
    syncedAt TIMESTAMP
);
CREATE INDEX IF NOT EXISTS WatchHistory_startedAt ON WatchHistory(startedAt);
CREATE INDEX IF NOT EXISTS WatchHistory_channel_started ON WatchHistory(channelId, startedAt);
CREATE TABLE IF NOT EXISTS CachedImageEntry (
    id INTEGER PRIMARY KEY,
    urlHash TEXT NOT NULL UNIQUE,
    urlString TEXT,
    relativePath TEXT NOT NULL,
    etag TEXT,
    lastModified TEXT,
    mimeType TEXT,
    sizeBytes INTEGER NOT NULL,
    fetchedAt TIMESTAMP NOT NULL,
    accessedAt TIMESTAMP NOT NULL,
    accessCount INTEGER NOT NULL,
    expiresAt TIMESTAMP,
    source TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS CachedImageEntry_accessedAt ON CachedImageEntry(accessedAt);
CREATE TABLE IF NOT EXISTS CachedLiveChannel (
    id INTEGER PRIMARY KEY,
    channelId TEXT NOT NULL UNIQUE,
    channelName TEXT NOT NULL,
    channelImageUrl TEXT,
    liveTitle TEXT NOT NULL,
    viewerCount INTEGER NOT NULL,
    categoryName TEXT,
    categoryType TEXT,
    thumbnailUrl TEXT,
    isLive BOOLEAN NOT NULL,
    openDate TIMESTAMP,
    sources TEXT NOT NULL,
    fetchedAt TIMESTAMP NOT NULL
);
CREATE INDEX IF NOT EXISTS CachedLiveChannel_fetchedAt ON CachedLiveChannel(fetchedAt);
CREATE TABLE IF NOT EXISTS CachedClip (
    id INTEGER PRIMARY KEY,
    clipUID TEXT NOT NULL UNIQUE,
    clipTitle TEXT NOT NULL,
    thumbnailImageURL TEXT,
    clipURL TEXT,
    duration INTEGER NOT NULL,
    readCount INTEGER NOT NULL,
    createdDate TIMESTAMP,
    channelId TEXT,
    channelName TEXT,
    channelImageURL TEXT,
    contentHash TEXT NOT NULL,
    sources TEXT NOT NULL,
    fetchedAt TIMESTAMP NOT NULL
);
CREATE INDEX IF NOT EXISTS CachedClip_fetchedAt ON CachedClip(fetchedAt);
CREATE TABLE IF NOT EXISTS CachedCategoryStat (
    id INTEGER PRIMARY KEY,
    categoryName TEXT NOT NULL UNIQUE,
    liveCount INTEGER NOT NULL,
    totalViewers INTEGER NOT NULL,
    topChannelId TEXT,
    topChannelName TEXT,
    fetchedAt TIMESTAMP NOT NULL
);
CREATE TABLE IF NOT EXISTS StreamBookmark (
    id INTEGER PRIMARY KEY,
    channelId TEXT NOT NULL,
    channelName TEXT NOT NULL,
    liveTitle TEXT NOT NULL,
    categoryName TEXT,
    thumbnailURL TEXT,
    capturedAt TIMESTAMP NOT NULL,
    liveUptime REAL NOT NULL,
    note TEXT
);
CREATE INDEX IF NOT EXISTS StreamBookmark_capturedAt ON StreamBookmark(capturedAt);
"#;

/// `UserSchemaV1` — 5개 엔티티 추가.
const V2_USER_SCHEMA_V1_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS CachedVOD (
    id INTEGER PRIMARY KEY,
    videoNo INTEGER NOT NULL UNIQUE,
    videoId TEXT,
    videoTitle TEXT NOT NULL,
    thumbnailImageURL TEXT,
    duration INTEGER NOT NULL,
    readCount INTEGER NOT NULL,
    publishDate TIMESTAMP,
    channelId TEXT,
    channelName TEXT,
    channelImageURL TEXT,
    contentHash TEXT NOT NULL,
    sources TEXT NOT NULL,
    fetchedAt TIMESTAMP NOT NULL
);
CREATE INDEX IF NOT EXISTS CachedVOD_fetchedAt ON CachedVOD(fetchedAt);
CREATE TABLE IF NOT EXISTS CategorySnapshot (
    id INTEGER PRIMARY KEY,
    category TEXT NOT NULL,
    viewerTotal INTEGER NOT NULL,
    liveCount INTEGER NOT NULL,
    capturedAt TIMESTAMP NOT NULL
);
CREATE INDEX IF NOT EXISTS CategorySnapshot_capturedAt ON CategorySnapshot(capturedAt);
CREATE INDEX IF NOT EXISTS CategorySnapshot_category ON CategorySnapshot(category, capturedAt);
CREATE TABLE IF NOT EXISTS PersistedClipPlayback (
    id INTEGER PRIMARY KEY,
    clipUID TEXT NOT NULL UNIQUE,
    lastPositionSec REAL NOT NULL,
    durationSec REAL NOT NULL,
    clipTitle TEXT NOT NULL,
    channelName TEXT NOT NULL,
    thumbnailURLString TEXT,
    lastSeenAt TIMESTAMP NOT NULL
);
CREATE INDEX IF NOT EXISTS PersistedClipPlayback_lastSeenAt ON PersistedClipPlayback(lastSeenAt);
CREATE TABLE IF NOT EXISTS PersistedVODPlayback (
    id INTEGER PRIMARY KEY,
    videoNo INTEGER NOT NULL UNIQUE,
    lastPositionSec REAL NOT NULL,
    durationSec REAL NOT NULL,
    videoTitle TEXT NOT NULL,
    channelName TEXT NOT NULL,
    thumbnailURLString TEXT,
    lastSeenAt TIMESTAMP NOT NULL
);
CREATE INDEX IF NOT EXISTS PersistedVODPlayback_lastSeenAt ON PersistedVODPlayback(lastSeenAt);
CREATE TABLE IF NOT EXISTS PersistedVODWatchLater (
    id INTEGER PRIMARY KEY,
    videoNo INTEGER NOT NULL UNIQUE,
    addedAt TIMESTAMP NOT NULL,
    videoTitle TEXT NOT NULL,
    channelName TEXT NOT NULL,
    thumbnailURLString TEXT,
    durationSec REAL NOT NULL
);
"#;

/// 버전 순 마이그레이션 목록(`UserMigrationPlan.schemas` + `stages` 대응).
pub const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        description: "2026-05-10 레거시 스토어(9개 모델)",
        sql: V1_LEGACY_SQL,
    },
    Migration {
        version: 2,
        description: "UserSchemaV1(14개 모델) — 엔티티 5개 추가(경량)",
        sql: V2_USER_SCHEMA_V1_SQL,
    },
];

/// 연결별 PRAGMA. 파일 스토어는 WAL, 모두 외래 키 사용.
pub(crate) fn configure(conn: &Connection, file_backed: bool) -> Result<()> {
    conn.pragma_update(None, "foreign_keys", true)?;
    if file_backed {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
    }
    Ok(())
}

/// 현재 `user_version`.
pub fn user_version(conn: &Connection) -> Result<i64> {
    Ok(conn.pragma_query_value(None, "user_version", |r| r.get(0))?)
}

/// 미적용 마이그레이션을 순서대로 적용한다. 적용 후 버전을 돌려준다.
///
/// 빈 스토어는 테이블을 만들기 전에 `auto_vacuum = INCREMENTAL` 을 켠다(Core Data 스토어와 같은 설정 —
/// `DataStore+StoreMaintenance.swift` 주석). 이 빌드보다 새 버전이면 [`PersistenceError::NewerSchema`].
pub fn migrate(conn: &mut Connection) -> Result<i64> {
    let current = user_version(conn)?;
    if current > CURRENT_SCHEMA_VERSION {
        return Err(PersistenceError::NewerSchema {
            found: current,
            supported: CURRENT_SCHEMA_VERSION,
        });
    }
    if current == 0 {
        // 테이블이 없을 때만 효과가 있다(이미 페이지가 있으면 VACUUM 이 필요).
        conn.pragma_update(None, "auto_vacuum", "INCREMENTAL")?;
    }
    for migration in MIGRATIONS.iter().filter(|m| m.version > current) {
        let tx = conn.transaction()?;
        tx.execute_batch(migration.sql)?;
        tx.pragma_update(None, "user_version", migration.version)?;
        tx.commit()?;
    }
    user_version(conn)
}

/// 지정 버전까지만 마이그레이션한다 — 업그레이드 경로 테스트용(레거시 스토어 합성).
pub fn migrate_to(conn: &mut Connection, target: i64) -> Result<i64> {
    let current = user_version(conn)?;
    if current == 0 {
        conn.pragma_update(None, "auto_vacuum", "INCREMENTAL")?;
    }
    for migration in MIGRATIONS
        .iter()
        .filter(|m| m.version > current && m.version <= target)
    {
        let tx = conn.transaction()?;
        tx.execute_batch(migration.sql)?;
        tx.pragma_update(None, "user_version", migration.version)?;
        tx.commit()?;
    }
    user_version(conn)
}

// MARK: - 사전 점검

/// 컨테이너를 열기 전에 스토어 메타데이터만 읽어 목표 스키마와 맞는지 판정한다(읽기 전용).
///
/// 원본: `StoreOpenHealth.swift` 의 `StorePreflight`. CoreData 메타데이터 호환성 대신
/// `PRAGMA user_version` 을 비교한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StorePreflight {
    /// 파일이 없다 — 첫 실행.
    Missing,
    /// 목표 스키마와 같다 — 평소 실행. 백업하지 않는다.
    MatchesTarget,
    /// 열면 마이그레이션이 일어난다 — 열기 전에 백업한다.
    NeedsLightweightUpgrade,
    /// 메타데이터를 읽지 못했다(손상·쓰레기 바이트·권한·더 새 버전).
    Unreadable,
}

impl StorePreflight {
    /// Swift `rawValue`.
    pub fn raw_value(self) -> &'static str {
        match self {
            StorePreflight::Missing => "missing",
            StorePreflight::MatchesTarget => "matchesTarget",
            StorePreflight::NeedsLightweightUpgrade => "needsLightweightUpgrade",
            StorePreflight::Unreadable => "unreadable",
        }
    }

    /// 스토어를 열거나 바꾸지 않는다.
    pub fn check(store_path: &Path) -> Self {
        Self::inspect(store_path).0
    }

    /// `check` 와 같고, 읽지 못했을 때의 오류를 함께 돌려준다.
    pub fn inspect(store_path: &Path) -> (Self, Option<PersistenceError>) {
        if !store_path.exists() {
            return (StorePreflight::Missing, None);
        }
        let conn = match Connection::open_with_flags(
            store_path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        ) {
            Ok(c) => c,
            Err(e) => return (StorePreflight::Unreadable, Some(e.into())),
        };
        // 쓰레기 바이트는 첫 조회에서 SQLITE_NOTADB(26) 로 드러난다.
        let tables: rusqlite::Result<i64> = conn.query_row(
            "SELECT count(*) FROM sqlite_master WHERE type = 'table'",
            [],
            |r| r.get(0),
        );
        let tables = match tables {
            Ok(n) => n,
            Err(e) => return (StorePreflight::Unreadable, Some(e.into())),
        };
        let version = match user_version(&conn) {
            Ok(v) => v,
            Err(e) => return (StorePreflight::Unreadable, Some(e)),
        };
        if tables == 0 && version == 0 {
            // 빈 파일 — CoreData 메타데이터가 없는 것과 같다.
            return (StorePreflight::Unreadable, None);
        }
        match version {
            v if v == CURRENT_SCHEMA_VERSION => (StorePreflight::MatchesTarget, None),
            v if v < CURRENT_SCHEMA_VERSION => (StorePreflight::NeedsLightweightUpgrade, None),
            v => (
                StorePreflight::Unreadable,
                Some(PersistenceError::NewerSchema {
                    found: v,
                    supported: CURRENT_SCHEMA_VERSION,
                }),
            ),
        }
    }
}

// MARK: - 스키마 모양(골든 비교용)

/// 엔티티 관계(SQL 외래 키로는 다 드러나지 않는 모양 정보). `SchemaShape.Relationship` 대응.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelationshipShape {
    pub name: String,
    pub destination: String,
    pub to_many: bool,
    pub optional: bool,
    /// NSDeleteRule — 1 nullify, 2 cascade.
    pub delete_rule: u32,
}

/// 속성 모양. `type` 은 CoreData `NSAttributeType` 코드.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttributeShape {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: u32,
    pub optional: bool,
    pub has_default: bool,
}

/// 엔티티 모양.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EntityShape {
    pub name: String,
    pub attributes: Vec<AttributeShape>,
    pub relationships: Vec<RelationshipShape>,
}

/// 스키마 모양(엔티티·속성·관계를 이름순 정렬). 테스트 골든(`user-schema-v1.shape.json`) 형식.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SchemaShape {
    pub entities: Vec<EntityShape>,
}

fn relationships_for(entity: &str) -> Vec<RelationshipShape> {
    match entity {
        "PersistedChannel" => vec![RelationshipShape {
            name: "statistics".into(),
            destination: "PersistedStatistic".into(),
            to_many: true,
            optional: true,
            delete_rule: 2,
        }],
        "PersistedStatistic" => vec![RelationshipShape {
            name: "channel".into(),
            destination: "PersistedChannel".into(),
            to_many: false,
            optional: true,
            delete_rule: 1,
        }],
        _ => vec![],
    }
}

fn type_code(declared: &str) -> u32 {
    match declared.to_ascii_uppercase().as_str() {
        "TEXT" => 700,
        "INTEGER" => 300,
        "REAL" => 500,
        "BOOLEAN" => 800,
        "TIMESTAMP" => 900,
        "BLOB" => 1000,
        _ => 0,
    }
}

impl SchemaShape {
    /// 열린 연결의 테이블에서 모양을 뽑는다. `id`(행 식별자)와 관계 열(외래 키)은 속성이 아니다.
    pub fn from_connection(conn: &Connection) -> Result<Self> {
        let mut names: Vec<String> = {
            let mut stmt = conn.prepare(
                "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
            )?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        names.sort();
        let mut entities = Vec::new();
        for name in names {
            let relationships = relationships_for(&name);
            let mut stmt = conn.prepare(&format!("PRAGMA table_info(\"{name}\")"))?;
            let mut attributes: Vec<AttributeShape> = stmt
                .query_map([], |r| {
                    Ok((
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, i64>(3)?,
                        r.get::<_, Option<String>>(4)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?
                .into_iter()
                .filter(|(col, ..)| col != "id" && !relationships.iter().any(|r| &r.name == col))
                .map(|(col, ty, notnull, default)| AttributeShape {
                    name: col,
                    kind: type_code(&ty),
                    optional: notnull == 0,
                    has_default: default.is_some(),
                })
                .collect();
            attributes.sort_by(|a, b| a.name.cmp(&b.name));
            entities.push(EntityShape {
                name,
                attributes,
                relationships,
            });
        }
        Ok(SchemaShape { entities })
    }

    /// 버전 없는 스토어 진화 규칙 위반 목록(`UnversionedEvolutionRule.violations`).
    /// 허용: 엔티티 추가·삭제, optional 이거나 기본값이 있는 속성 추가, 속성 삭제.
    pub fn evolution_violations(old: &SchemaShape, new: &SchemaShape) -> Vec<String> {
        let mut result = Vec::new();
        for entity in &new.entities {
            let Some(previous) = old.entities.iter().find(|e| e.name == entity.name) else {
                continue;
            };
            for attribute in &entity.attributes {
                if let Some(before) = previous
                    .attributes
                    .iter()
                    .find(|a| a.name == attribute.name)
                {
                    if before.kind != attribute.kind {
                        result.push(format!("{}.{}: 타입 변경", entity.name, attribute.name));
                    }
                    if before.optional && !attribute.optional {
                        result.push(format!(
                            "{}.{}: optional → 필수",
                            entity.name, attribute.name
                        ));
                    }
                } else if !attribute.optional && !attribute.has_default {
                    result.push(format!(
                        "{}.{}: 기본값 없는 필수 속성 추가",
                        entity.name, attribute.name
                    ));
                }
            }
            for relation in &entity.relationships {
                if let Some(before) = previous
                    .relationships
                    .iter()
                    .find(|r| r.name == relation.name)
                {
                    if before != relation {
                        result.push(format!("{}.{}: 관계 모양 변경", entity.name, relation.name));
                    }
                } else if !relation.optional && !relation.to_many {
                    result.push(format!("{}.{}: 필수 관계 추가", entity.name, relation.name));
                }
            }
        }
        result
    }
}
