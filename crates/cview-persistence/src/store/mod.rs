//! 통합 데이터 저장소.
//!
//! 원본: `Sources/CViewPersistence/DataStore.swift`(채널·메모·설정·통계·시청 기록 시작/종료) 와
//! `DataStore+SaveOrRollback.swift`. 도메인별 확장은 이 모듈의 하위 모듈로 옮겼다.
//!
//! Swift `@ModelActor actor DataStore` 는 `Arc` 로 공유하는 구조체 + `Mutex<rusqlite::Connection>` 이다.
//! 동기 메서드는 `&self` 로 부르고, 대량 작업은 [`DataStore::off_main`] 으로 블로킹 풀에서 돌린다.
//!
//! save/rollback: 쓰기는 [`DataStore::write`] 안의 트랜잭션에서 한다. 바뀐 것이 있을 때만 커밋하고,
//! 오류가 나면 트랜잭션을 버린다(`saveOrRollback` 과 같은 의미 — 실패한 변경이 다음 무관한 save 에
//! 섞여 커밋되지 않는다). 커밋마다 변경 이력(SwiftData persistent history 대응)을 한 건 남긴다.

pub mod bookmarks;
pub mod category_snapshots;
pub mod clip_cache;
pub mod clip_playback;
pub mod image_cache;
pub mod legacy_cleanup;
pub mod live_channel_cache;
pub mod maintenance;
pub mod off_main;
pub mod store_open;
pub mod vod_cache;
pub mod vod_playback;
pub mod vod_watch_later;
pub mod watch_history_sync;
pub mod watch_history_sync_queue;
pub mod watch_time_window;

use crate::db::{self, dv};
use crate::error::{PersistenceError, Result};
use crate::models::{
    ChannelListData, PersistedChannel, PersistedSetting, PersistedStatistic, WatchHistory,
};
use crate::revision::WatchInsightsRevision;
use crate::schema;
use crate::time::{adding, interval_since, now, Calendar, Date};
use cview_core::models::ChannelInfo;
use rusqlite::types::Value;
use rusqlite::{Connection, Transaction, TransactionBehavior};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, RwLock};

/// 변경 이력 보관 상한(프로세스 메모리 보호).
const HISTORY_CAP: usize = 100_000;

struct Inner {
    conn: Mutex<Connection>,
    /// 커밋 시각 기록 — SwiftData persistent history(`ATRANSACTION`) 대응.
    history: Mutex<Vec<Date>>,
    calendar: RwLock<Arc<dyn Calendar>>,
    /// 테스트용: 다음 커밋을 실패시킨다(`saveOrRollback(save:)` 실패 주입 seam).
    fail_next_commit: AtomicBool,
    path: Option<PathBuf>,
}

/// 통합 데이터 저장소. 복제는 같은 연결을 공유한다.
#[derive(Clone)]
pub struct DataStore {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for DataStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DataStore")
            .field("path", &self.inner.path)
            .finish()
    }
}

/// `SettingsStore` 등 호출자가 쓰는 설정 키 청크 크기(SQLite 바인딩 한계 회피).
pub const SETTING_KEY_CHUNK_SIZE: usize = 200;

impl DataStore {
    // MARK: - 열기

    fn from_connection(mut conn: Connection, path: Option<PathBuf>) -> Result<Self> {
        schema::configure(&conn, path.is_some())?;
        schema::migrate(&mut conn)?;
        Ok(Self {
            inner: Arc::new(Inner {
                conn: Mutex::new(conn),
                history: Mutex::new(Vec::new()),
                calendar: RwLock::new(Arc::new(chrono::Local)),
                fail_next_commit: AtomicBool::new(false),
                path,
            }),
        })
    }

    /// 테스트용 인메모리 저장소(`createInMemoryContainer`).
    pub fn open_in_memory() -> Result<Self> {
        Self::from_connection(Connection::open_in_memory()?, None)
    }

    /// 파일 스토어를 연다(없으면 만든다). 마이그레이션까지 적용한다.
    /// 사전 점검·백업·기록이 필요하면 [`DataStore::create_container_at`] 을 쓴다.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(dir) = path.parent() {
            if !dir.as_os_str().is_empty() {
                std::fs::create_dir_all(dir)?;
            }
        }
        Self::from_connection(Connection::open(&path)?, Some(path))
    }

    /// 스토어 파일 경로(인메모리면 `None`).
    pub fn path(&self) -> Option<&Path> {
        self.inner.path.as_deref()
    }

    /// 실사용 스토어 경로 — `<applicationSupport>/CView/CView_v2.store`.
    ///
    /// 주입 가능한 파일 연산은 권한 실패를 실제 사용자 저장소 변경 없이 검증하기 위함이다.
    /// 영속 경로를 확보하지 못하면 오류다(임시 DB 로 대체하지 않는다).
    pub fn resolve_store_url_with(
        application_support: impl FnOnce() -> Result<PathBuf>,
        create_directory: impl FnOnce(&Path) -> Result<()>,
    ) -> Result<PathBuf> {
        let base = application_support()?;
        let dir = base.join("CView");
        create_directory(&dir)?;
        Ok(dir.join("CView_v2.store"))
    }

    /// 기본 Application Support 를 쓰는 [`DataStore::resolve_store_url_with`].
    pub fn resolve_store_url() -> Result<PathBuf> {
        Self::resolve_store_url_with(application_support_dir, |dir| {
            std::fs::create_dir_all(dir).map_err(Into::into)
        })
    }

    /// 달력을 바꾼다(기본 `chrono::Local` — Swift `Calendar.current`). 테스트가 시간대를 고정할 때 쓴다.
    pub fn set_calendar(&self, calendar: Arc<dyn Calendar>) {
        *self
            .inner
            .calendar
            .write()
            .unwrap_or_else(|e| e.into_inner()) = calendar;
    }

    pub(crate) fn calendar(&self) -> Arc<dyn Calendar> {
        self.inner
            .calendar
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    // MARK: - 트랜잭션 보조

    fn lock(&self) -> MutexGuard<'_, Connection> {
        self.inner.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 읽기 전용 작업.
    pub(crate) fn read<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let conn = self.lock();
        f(&conn)
    }

    /// 쓰기 트랜잭션. 본문이 `(값, 바뀜)` 을 돌려주면 바뀐 경우에만 커밋한다.
    /// 본문 오류·커밋 실패는 트랜잭션을 버린다(saveOrRollback).
    pub(crate) fn write<T>(
        &self,
        f: impl FnOnce(&Transaction<'_>) -> Result<(T, bool)>,
    ) -> Result<T> {
        let mut conn = self.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (value, changed) = f(&tx)?;
        if changed {
            if self.inner.fail_next_commit.swap(false, Ordering::SeqCst) {
                drop(tx); // rollback
                return Err(PersistenceError::Other("주입된 저장 실패".into()));
            }
            tx.commit()?;
            self.record_history();
        }
        Ok(value)
    }

    fn record_history(&self) {
        let mut history = self.inner.history.lock().unwrap_or_else(|e| e.into_inner());
        history.push(now());
        if history.len() > HISTORY_CAP {
            let excess = history.len() - HISTORY_CAP;
            history.drain(..excess);
        }
    }

    /// 테스트용 실패 주입 — 다음 커밋 한 번을 실패시킨다.
    #[doc(hidden)]
    pub fn inject_commit_failure(&self) {
        self.inner.fail_next_commit.store(true, Ordering::SeqCst);
    }

    pub(crate) fn history(&self) -> MutexGuard<'_, Vec<Date>> {
        self.inner.history.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 원시 SQL 로 연결을 다룬다 — 진단·테스트 시드용.
    #[doc(hidden)]
    pub fn with_connection<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        self.read(f)
    }

    // MARK: - 채널

    fn channel_by_id(conn: &Connection, channel_id: &str) -> Result<Option<PersistedChannel>> {
        Ok(db::fetch_one::<PersistedChannel>(
            conn,
            "WHERE channelId = ?",
            &[Value::Text(channel_id.into())],
        )?)
    }

    /// 채널 저장/업데이트.
    ///
    /// ⚠ `is_favorite` 는 **신규 insert 에만** 반영된다(시청 시작 보강 경로가 기본값 false 로 부른다).
    /// 비어 있는 이미지 URL 로 기존 값을 덮어쓰지 않는다(CHAN-2).
    pub fn save_channel(&self, info: &ChannelInfo, is_favorite: bool) -> Result<()> {
        self.write(|tx| {
            if let Some(mut existing) = Self::channel_by_id(tx, &info.channel_id)? {
                existing.channel_name = info.channel_name.clone();
                if let Some(url) = info.channel_image_url.as_ref().map(|u| u.to_string()) {
                    if !url.is_empty() {
                        existing.image_url = Some(url);
                    }
                }
                db::update(tx, &existing)?;
            } else {
                let mut channel = PersistedChannel::new(
                    info.channel_id.clone(),
                    info.channel_name.clone(),
                    info.channel_image_url.as_ref().map(|u| u.to_string()),
                    is_favorite,
                    None,
                );
                db::insert(tx, &mut channel)?;
            }
            Ok(((), true))
        })
    }

    /// 즐겨찾기 값을 **명시적으로** 설정한다(토글이 아니다). row 가 없으면 `info` 로 만든다.
    /// `info` 도 없고 row 도 없으면 아무것도 하지 않고 `false`.
    pub fn set_favorite(
        &self,
        channel_id: &str,
        is_favorite: bool,
        info: Option<&ChannelInfo>,
    ) -> Result<bool> {
        self.write(|tx| {
            if let Some(mut existing) = Self::channel_by_id(tx, channel_id)? {
                if let Some(info) = info {
                    if !info.channel_name.is_empty() {
                        existing.channel_name = info.channel_name.clone();
                    }
                    if let Some(url) = info.channel_image_url.as_ref().map(|u| u.to_string()) {
                        if !url.is_empty() {
                            existing.image_url = Some(url);
                        }
                    }
                }
                existing.is_favorite = is_favorite;
                db::update(tx, &existing)?;
            } else if let Some(info) = info {
                let mut channel = PersistedChannel::new(
                    info.channel_id.clone(),
                    info.channel_name.clone(),
                    info.channel_image_url.as_ref().map(|u| u.to_string()),
                    is_favorite,
                    None,
                );
                db::insert(tx, &mut channel)?;
            } else {
                return Ok((false, false));
            }
            Ok((is_favorite, true))
        })
    }

    /// 즐겨찾기 토글. row 가 없으면 false.
    pub fn toggle_favorite(&self, channel_id: &str) -> Result<bool> {
        self.write(|tx| {
            let Some(mut channel) = Self::channel_by_id(tx, channel_id)? else {
                return Ok((false, false));
            };
            channel.is_favorite = !channel.is_favorite;
            db::update(tx, &channel)?;
            Ok((channel.is_favorite, true))
        })
    }

    /// 모든 즐겨찾기 해제.
    pub fn clear_all_favorites(&self) -> Result<()> {
        self.write(|tx| {
            let n = tx.execute(
                "UPDATE PersistedChannel SET isFavorite = 0 WHERE isFavorite = 1",
                [],
            )?;
            Ok(((), n > 0))
        })
    }

    /// 모든 채널의 "최근 시청" 흔적을 지운다. 인사이트 리비전은 올리지 않는다.
    pub fn clear_all_last_watched(&self) -> Result<()> {
        self.write(|tx| {
            let n = tx.execute(
                "UPDATE PersistedChannel SET lastWatched = NULL WHERE lastWatched IS NOT NULL",
                [],
            )?;
            Ok(((), n > 0))
        })
    }

    /// 최근 시청 시각을 지금으로.
    pub fn update_last_watched(&self, channel_id: &str) -> Result<()> {
        self.write(|tx| {
            let Some(mut channel) = Self::channel_by_id(tx, channel_id)? else {
                return Ok(((), false));
            };
            channel.last_watched = Some(now());
            db::update(tx, &channel)?;
            Ok(((), true))
        })
    }

    /// 즐겨찾기 여부.
    pub fn is_favorite(&self, channel_id: &str) -> Result<bool> {
        self.read(|conn| Ok(Self::channel_by_id(conn, channel_id)?.is_some_and(|c| c.is_favorite)))
    }

    /// 즐겨찾기 목록 — 채널 이름순(`localizedStandard` 비교: 대소문자 무시·숫자 인식).
    pub fn fetch_favorite_items(&self) -> Result<Vec<ChannelListData>> {
        self.read(|conn| {
            let mut rows = db::fetch::<PersistedChannel>(conn, "WHERE isFavorite = 1", &[])?;
            rows.sort_by(|a, b| localized_standard_cmp(&a.channel_name, &b.channel_name));
            Ok(rows.iter().map(ChannelListData::from).collect())
        })
    }

    /// 최근 시청 목록 — 최근순 limit 개.
    pub fn fetch_recent_items(&self, limit: usize) -> Result<Vec<ChannelListData>> {
        self.read(|conn| {
            let rows = db::fetch::<PersistedChannel>(
                conn,
                "WHERE lastWatched IS NOT NULL ORDER BY lastWatched DESC, id ASC LIMIT ?",
                &[Value::Integer(limit as i64)],
            )?;
            Ok(rows.iter().map(ChannelListData::from).collect())
        })
    }

    /// 채널 행 전체(진단·테스트).
    pub fn fetch_channel(&self, channel_id: &str) -> Result<Option<PersistedChannel>> {
        self.read(|conn| Self::channel_by_id(conn, channel_id))
    }

    // MARK: - 메모

    /// 채널 메모 저장(빈 문자열은 nil). row 가 없으면 아무것도 하지 않는다.
    pub fn save_memo(&self, channel_id: &str, memo: &str) -> Result<()> {
        self.write(|tx| {
            let Some(mut channel) = Self::channel_by_id(tx, channel_id)? else {
                return Ok(((), false));
            };
            channel.memo = if memo.is_empty() {
                None
            } else {
                Some(memo.to_string())
            };
            db::update(tx, &channel)?;
            Ok(((), true))
        })
    }

    /// 채널 메모 조회(없으면 빈 문자열).
    pub fn fetch_memo(&self, channel_id: &str) -> Result<String> {
        self.read(|conn| {
            Ok(Self::channel_by_id(conn, channel_id)?
                .and_then(|c| c.memo)
                .unwrap_or_default())
        })
    }

    // MARK: - 설정

    /// 설정 저장 — 키 정렬 JSON. 같은 바이트면 쓰지 않는다.
    pub fn save_setting<T: Serialize + ?Sized>(&self, key: &str, value: &T) -> Result<()> {
        let data = crate::json::to_sorted_vec(value)?;
        let mut pairs = HashMap::new();
        pairs.insert(key.to_string(), data);
        self.save_settings(&pairs)
    }

    /// 여러 설정 키를 한 트랜잭션으로 저장한다. 바뀐 행이 없으면 커밋하지 않는다.
    pub fn save_settings(&self, pairs: &HashMap<String, Vec<u8>>) -> Result<()> {
        if pairs.is_empty() {
            return Ok(());
        }
        self.write(|tx| {
            let keys: HashSet<String> = pairs.keys().cloned().collect();
            let mut existing = Self::fetch_setting_rows(tx, &keys)?;
            let at = now();
            let mut changed = false;
            for (key, data) in pairs {
                if let Some(row) = existing.get_mut(key) {
                    if &row.value_data == data {
                        continue;
                    }
                    row.replace_value(data.clone(), at);
                    db::update(tx, row)?;
                } else {
                    let mut row = PersistedSetting::new(key.clone(), data.clone());
                    db::insert(tx, &mut row)?;
                }
                changed = true;
            }
            Ok(((), changed))
        })
    }

    /// 여러 설정 키의 원시 JSON 바이트를 한 번에 읽는다.
    pub fn load_settings_raw(&self, keys: &HashSet<String>) -> Result<HashMap<String, Vec<u8>>> {
        if keys.is_empty() {
            return Ok(HashMap::new());
        }
        self.read(|conn| {
            Ok(Self::fetch_setting_rows(conn, keys)?
                .into_iter()
                .map(|(k, v)| (k, v.value_data))
                .collect())
        })
    }

    /// 주어진 키의 설정 행을 지운다. 없는 키는 건드리지 않는다.
    pub fn delete_settings(&self, keys: &HashSet<String>) -> Result<()> {
        self.write(|tx| {
            let rows = Self::fetch_setting_rows(tx, keys)?;
            for row in rows.values() {
                db::delete(tx, row)?;
            }
            Ok(((), !rows.is_empty()))
        })
    }

    fn fetch_setting_rows(
        conn: &Connection,
        keys: &HashSet<String>,
    ) -> Result<HashMap<String, PersistedSetting>> {
        let values: Vec<Value> = keys.iter().map(|k| Value::Text(k.clone())).collect();
        Ok(
            db::fetch_in::<PersistedSetting>(conn, "key", &values, SETTING_KEY_CHUNK_SIZE)?
                .into_iter()
                .map(|r| (r.key.clone(), r))
                .collect(),
        )
    }

    /// 설정 읽기. 키가 없으면 `None`, 디코드 실패는 오류.
    pub fn load_setting<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        let row = self.read(|conn| {
            Ok(db::fetch_one::<PersistedSetting>(
                conn,
                "WHERE key = ?",
                &[Value::Text(key.into())],
            )?)
        })?;
        match row {
            None => Ok(None),
            Some(row) => Ok(Some(serde_json::from_slice(&row.value_data)?)),
        }
    }

    /// `keys` 중 저장된 행이 하나라도 있는지(읽기 실패는 false). `cache.*` 행은 세지 않는다 — 키 목록으로만 본다.
    pub fn has_any_setting(&self, keys: &[String]) -> bool {
        if keys.is_empty() {
            return false;
        }
        let values: Vec<Value> = keys.iter().map(|k| Value::Text(k.clone())).collect();
        self.read(|conn| {
            for chunk in values.chunks(SETTING_KEY_CHUNK_SIZE) {
                let n = db::count::<PersistedSetting>(
                    conn,
                    &format!("WHERE key IN ({})", db::placeholders(chunk.len())),
                    chunk,
                )?;
                if n > 0 {
                    return Ok(true);
                }
            }
            Ok(false)
        })
        .unwrap_or(false)
    }

    // MARK: - 통계

    /// 통계 기록 저장.
    pub fn save_statistic(
        &self,
        channel_id: &str,
        viewer_count: i64,
        latency: Option<f64>,
    ) -> Result<()> {
        self.write(|tx| {
            let channel = Self::channel_by_id(tx, channel_id)?.map(|c| c.id);
            let mut stat = PersistedStatistic {
                id: 0,
                channel_id: channel_id.to_string(),
                timestamp: now(),
                viewer_count,
                watch_duration: 0.0,
                average_latency: latency,
                channel,
            };
            db::insert(tx, &mut stat)?;
            Ok(((), true))
        })
    }

    // MARK: - 시청 기록

    /// 시청 기록 시작. 최근 시청 채널 row 도 같은 트랜잭션에서 보강한다.
    ///
    /// ⚠ `thumbnail_url` 은 라이브 프레임이지 채널 아바타가 아니다 — `PersistedChannel.imageURL` 에 쓰지 않는다.
    pub fn start_watch_record(
        &self,
        channel_id: &str,
        channel_name: &str,
        thumbnail_url: Option<&str>,
        category_name: Option<&str>,
        started_at: Date,
    ) -> Result<Date> {
        let started = self.write(|tx| {
            let mut record = WatchHistory::new(
                channel_id,
                channel_name,
                thumbnail_url.map(str::to_string),
                category_name.map(str::to_string),
                started_at,
            );
            db::insert(tx, &mut record)?;
            if let Some(mut channel) = Self::channel_by_id(tx, channel_id)? {
                channel.channel_name = channel_name.to_string();
                channel.last_watched = Some(started_at);
                db::update(tx, &channel)?;
            } else {
                let mut channel =
                    PersistedChannel::new(channel_id, channel_name, None, false, Some(started_at));
                db::insert(tx, &mut channel)?;
            }
            Ok((record.started_at, true))
        })?;
        WatchInsightsRevision::bump();
        Ok(started)
    }

    /// 호출자가 받은 startedAt 과 **정확히 같은** 열린 시청 기록(±1초 창은 후보를 좁히는 용도).
    fn open_watch_record(
        conn: &Connection,
        channel_id: &str,
        started_at: &Date,
    ) -> Result<Option<WatchHistory>> {
        let lower = adding(started_at, -1.0);
        let upper = adding(started_at, 1.0);
        let rows = db::fetch::<WatchHistory>(
            conn,
            "WHERE channelId = ? AND endedAt IS NULL AND startedAt >= ? AND startedAt <= ?",
            &[Value::Text(channel_id.into()), dv(&lower), dv(&upper)],
        )?;
        Ok(rows.into_iter().find(|r| r.started_at == *started_at))
    }

    /// 진행 중인 시청 기록의 duration 을 저장한다. 못 찾으면 false(리비전 그대로).
    pub fn checkpoint_watch_record(
        &self,
        channel_id: &str,
        started_at: Date,
        checkpoint: Date,
    ) -> Result<bool> {
        let found = self.write(|tx| {
            let Some(mut record) = Self::open_watch_record(tx, channel_id, &started_at)? else {
                return Ok((false, false));
            };
            record.duration = record
                .duration
                .max(interval_since(&checkpoint, &record.started_at).max(0.0));
            record.updated_at = Some(checkpoint);
            record.synced_at = None;
            db::update(tx, &record)?;
            Ok((true, true))
        })?;
        if found {
            WatchInsightsRevision::bump();
        }
        Ok(found)
    }

    /// 시청 기록 종료. 못 찾으면 false.
    pub fn end_watch_record(
        &self,
        channel_id: &str,
        started_at: Date,
        ended_at: Date,
    ) -> Result<bool> {
        let found = self.write(|tx| {
            let Some(mut record) = Self::open_watch_record(tx, channel_id, &started_at)? else {
                return Ok((false, false));
            };
            record.ended_at = Some(ended_at);
            record.duration = record
                .duration
                .max(interval_since(&ended_at, &record.started_at).max(0.0));
            record.updated_at = Some(ended_at);
            record.synced_at = None;
            db::update(tx, &record)?;
            Ok((true, true))
        })?;
        if found {
            WatchInsightsRevision::bump();
        }
        Ok(found)
    }

    /// 테스트·시드용: 시청 기록 행을 그대로 넣는다(리비전은 올리지 않는다).
    #[doc(hidden)]
    pub fn insert_watch_history_rows(&self, rows: &[WatchHistory]) -> Result<()> {
        self.write(|tx| {
            for row in rows {
                let mut row = row.clone();
                db::insert(tx, &mut row)?;
            }
            Ok(((), !rows.is_empty()))
        })
    }

    /// 테스트·진단용: 시청 기록 전 행(시작 시각순).
    #[doc(hidden)]
    pub fn all_watch_history_rows(&self) -> Result<Vec<WatchHistory>> {
        self.read(|conn| {
            Ok(db::fetch::<WatchHistory>(
                conn,
                "ORDER BY startedAt ASC, id ASC",
                &[],
            )?)
        })
    }

    /// 테이블 행 수(진단·테스트).
    #[doc(hidden)]
    pub fn table_row_count(&self, table: &str) -> Result<i64> {
        if !schema::USER_SCHEMA_V1_MODELS.contains(&table) {
            return Err(PersistenceError::Other(format!(
                "알 수 없는 테이블: {table}"
            )));
        }
        self.read(|conn| {
            Ok(conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))?)
        })
    }
}

/// `String.localizedStandardCompare` 근사 — 대소문자 무시, 숫자 구간은 수 값으로 비교.
pub fn localized_standard_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let mut ai = a.chars().peekable();
    let mut bi = b.chars().peekable();
    loop {
        match (ai.peek().copied(), bi.peek().copied()) {
            (None, None) => return a.cmp(b),
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let mut na = String::new();
                while let Some(c) = ai.peek().copied().filter(char::is_ascii_digit) {
                    na.push(c);
                    ai.next();
                }
                let mut nb = String::new();
                while let Some(c) = bi.peek().copied().filter(char::is_ascii_digit) {
                    nb.push(c);
                    bi.next();
                }
                let ta = na.trim_start_matches('0');
                let tb = nb.trim_start_matches('0');
                let ord = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb));
                if ord != Ordering::Equal {
                    return ord;
                }
            }
            (Some(x), Some(y)) => {
                let fx: String = x.to_lowercase().collect();
                let fy: String = y.to_lowercase().collect();
                let ord = fx.cmp(&fy);
                if ord != Ordering::Equal {
                    return ord;
                }
                ai.next();
                bi.next();
            }
        }
    }
}

/// `FileManager.url(for: .applicationSupportDirectory)` 대응.
/// `CVIEW_APPLICATION_SUPPORT` → macOS `~/Library/Application Support` → `$XDG_DATA_HOME` → `~/.local/share`.
pub fn application_support_dir() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("CVIEW_APPLICATION_SUPPORT") {
        return Ok(PathBuf::from(dir));
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if cfg!(target_os = "macos") {
        if let Some(home) = home {
            return Ok(home.join("Library").join("Application Support"));
        }
    }
    if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
        return Ok(PathBuf::from(xdg));
    }
    home.map(|h| h.join(".local").join("share")).ok_or_else(|| {
        PersistenceError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Application Support 디렉터리를 찾을 수 없습니다",
        ))
    })
}
