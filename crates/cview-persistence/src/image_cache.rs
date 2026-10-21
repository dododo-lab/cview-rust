//! 썸네일·아바타 디스크 캐시 — 이미지 바이트는 파일(blob)로, 목록은 SQLite 색인으로 둔다.
//!
//! - 키: URL 의 SHA-256(16진). 같은 URL 은 한 파일.
//! - 신선도는 종류([`ImageKind`])마다 다르다: 라이브 썸네일은 CDN 이 `max-age=5`·ETag 없음이라 짧게,
//!   아바타·포스터는 길게. 오래돼도 지우지 않고 돌려준다 — 화면엔 바로 보이고 새로 받기는 호출자 몫(stale-while-revalidate).
//! - 크기 상한을 넘으면 가장 오래 안 쓴 것부터 지운다(LRU).
//! - 쿠키·토큰은 담지 않는다: 저장하는 것은 URL·형식·검증 헤더(ETag/Last-Modified)·바이트뿐이다.
//!
//! 이 모듈은 받기를 하지 않는다(네트워크는 앱 쪽 `image_service`).

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};

/// 이미지 종류 — 신선도(TTL)가 다르다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageKind {
    /// 방송 중 썸네일(같은 URL 이 계속 바뀐다).
    Live,
    /// 채널·사용자 아바타.
    Avatar,
    /// 카테고리 포스터·다시보기·클립 썸네일 등 잘 안 바뀌는 그림.
    Static,
}

impl ImageKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ImageKind::Live => "live",
            ImageKind::Avatar => "avatar",
            ImageKind::Static => "static",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "live" | "hero" => ImageKind::Live,
            "avatar" | "avatar-lg" => ImageKind::Avatar,
            _ => ImageKind::Static,
        }
    }

    /// 이만큼 지나면 다시 받아 바꾼다(그 사이엔 받지 않고 그대로 쓴다).
    pub fn ttl_ms(self) -> i64 {
        match self {
            ImageKind::Live => 45_000,
            ImageKind::Avatar | ImageKind::Static => 7 * 24 * 3_600_000,
        }
    }
}

/// 캐시된 이미지 한 장.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedImage {
    pub bytes: Vec<u8>,
    pub content_type: String,
    pub kind: ImageKind,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    /// 마지막으로 서버에서 받은(또는 304 로 확인한) 시각, 유닉스 ms.
    pub fetched_at: i64,
}

impl CachedImage {
    pub fn is_fresh(&self, now_ms: i64) -> bool {
        now_ms - self.fetched_at < self.kind.ttl_ms()
    }
}

/// 넣을 때의 메타데이터.
#[derive(Debug, Clone, Default)]
pub struct ImageMeta {
    pub content_type: String,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}

/// 캐시 상태(설정 화면 표시용).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheStats {
    pub entries: u64,
    pub bytes: u64,
    pub cap_bytes: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum ImageCacheError {
    #[error("이미지 캐시 데이터베이스 오류: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("이미지 캐시 파일 오류: {0}")]
    Io(#[from] std::io::Error),
}

pub type CacheResult<T> = std::result::Result<T, ImageCacheError>;

/// 기본 크기 상한 256MB.
pub const DEFAULT_CAP_BYTES: u64 = 256 * 1024 * 1024;
/// 한 장의 상한 — 이보다 크면 담지 않는다(잘못된 응답이 캐시를 다 밀어내지 않게).
pub const MAX_ENTRY_BYTES: usize = 8 * 1024 * 1024;

pub struct ImageCache {
    dir: PathBuf,
    db: Mutex<Connection>,
    cap_bytes: u64,
}

impl ImageCache {
    /// `dir` 아래에 `index.sqlite` 와 `blobs/` 를 둔다. 색인을 열지 못하면(손상) 새로 만든다.
    pub fn open(dir: &Path, cap_bytes: u64) -> CacheResult<Self> {
        fs::create_dir_all(dir.join("blobs"))?;
        let path = dir.join("index.sqlite");
        let db = match Self::open_db(&path) {
            Ok(db) => db,
            Err(e) => {
                // 색인은 다시 만들 수 있다 — 손상이면 지우고(파일도 함께) 새로 시작한다.
                tracing::warn!("이미지 캐시 색인을 새로 만듭니다: {e}");
                let _ = fs::remove_file(&path);
                let _ = fs::remove_dir_all(dir.join("blobs"));
                fs::create_dir_all(dir.join("blobs"))?;
                Self::open_db(&path)?
            }
        };
        Ok(Self {
            dir: dir.to_path_buf(),
            db: Mutex::new(db),
            cap_bytes: cap_bytes.max(1),
        })
    }

    fn open_db(path: &Path) -> CacheResult<Connection> {
        let db = Connection::open(path)?;
        db.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             CREATE TABLE IF NOT EXISTS images (
                key TEXT PRIMARY KEY,
                url TEXT NOT NULL,
                kind TEXT NOT NULL,
                content_type TEXT NOT NULL,
                etag TEXT,
                last_modified TEXT,
                size INTEGER NOT NULL,
                fetched_at INTEGER NOT NULL,
                last_access INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS images_last_access ON images(last_access);",
        )?;
        Ok(db)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.db.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn key(url: &str) -> String {
        let digest = Sha256::digest(url.as_bytes());
        digest.iter().map(|b| format!("{b:02x}")).collect()
    }

    fn blob_path(&self, key: &str) -> PathBuf {
        // 한 폴더에 수만 개가 몰리지 않게 앞 두 글자로 나눈다.
        self.dir.join("blobs").join(&key[..2]).join(key)
    }

    /// 캐시된 이미지(오래됐어도 돌려준다). 마지막 사용 시각을 갱신한다. 파일이 없어졌으면 색인도 지운다.
    pub fn get(&self, url: &str, now_ms: i64) -> CacheResult<Option<CachedImage>> {
        let key = Self::key(url);
        let row = {
            let db = self.lock();
            db.query_row(
                "SELECT kind, content_type, etag, last_modified, fetched_at FROM images WHERE key = ?1",
                params![key],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, Option<String>>(2)?,
                        r.get::<_, Option<String>>(3)?,
                        r.get::<_, i64>(4)?,
                    ))
                },
            )
            .optional()?
        };
        let Some((kind, content_type, etag, last_modified, fetched_at)) = row else {
            return Ok(None);
        };
        let bytes = match fs::read(self.blob_path(&key)) {
            Ok(b) => b,
            Err(_) => {
                self.lock()
                    .execute("DELETE FROM images WHERE key = ?1", params![key])?;
                return Ok(None);
            }
        };
        self.lock().execute(
            "UPDATE images SET last_access = ?2 WHERE key = ?1",
            params![key, now_ms],
        )?;
        Ok(Some(CachedImage {
            bytes,
            content_type,
            kind: ImageKind::parse(&kind),
            etag,
            last_modified,
            fetched_at,
        }))
    }

    /// 넣거나 바꾼다. 파일은 임시 이름으로 쓴 뒤 옮겨, 읽는 쪽이 반쯤 쓴 파일을 보지 않게 한다.
    /// 상한을 넘으면 오래 안 쓴 것부터 지운다.
    pub fn put(
        &self,
        url: &str,
        kind: ImageKind,
        meta: &ImageMeta,
        bytes: &[u8],
        now_ms: i64,
    ) -> CacheResult<()> {
        if bytes.is_empty() || bytes.len() > MAX_ENTRY_BYTES {
            return Ok(());
        }
        let key = Self::key(url);
        let path = self.blob_path(&key);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension(format!("tmp{}", std::process::id()));
        {
            let mut f = fs::File::create(&tmp)?;
            f.write_all(bytes)?;
        }
        fs::rename(&tmp, &path)?;
        self.lock().execute(
            "INSERT INTO images (key, url, kind, content_type, etag, last_modified, size, fetched_at, last_access)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)
             ON CONFLICT(key) DO UPDATE SET kind = ?3, content_type = ?4, etag = ?5, last_modified = ?6,
               size = ?7, fetched_at = ?8, last_access = ?8",
            params![
                key,
                url,
                kind.as_str(),
                meta.content_type,
                meta.etag,
                meta.last_modified,
                bytes.len() as i64,
                now_ms
            ],
        )?;
        self.evict_to_cap()?;
        Ok(())
    }

    /// 서버가 304(바뀌지 않음)로 답했다 — 받은 시각만 새로 한다.
    pub fn mark_fresh(&self, url: &str, now_ms: i64) -> CacheResult<()> {
        self.lock().execute(
            "UPDATE images SET fetched_at = ?2, last_access = ?2 WHERE key = ?1",
            params![Self::key(url), now_ms],
        )?;
        Ok(())
    }

    pub fn stats(&self) -> CacheResult<CacheStats> {
        let (entries, bytes) = self.lock().query_row(
            "SELECT COUNT(*), COALESCE(SUM(size), 0) FROM images",
            [],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
        )?;
        Ok(CacheStats {
            entries: entries.max(0) as u64,
            bytes: bytes.max(0) as u64,
            cap_bytes: self.cap_bytes,
        })
    }

    /// 전부 지운다(설정의 "썸네일 캐시 비우기").
    pub fn clear(&self) -> CacheResult<()> {
        self.lock().execute("DELETE FROM images", [])?;
        let blobs = self.dir.join("blobs");
        let _ = fs::remove_dir_all(&blobs);
        fs::create_dir_all(&blobs)?;
        Ok(())
    }

    fn evict_to_cap(&self) -> CacheResult<()> {
        let total = self.stats()?.bytes;
        if total <= self.cap_bytes {
            return Ok(());
        }
        // 상한의 90% 까지 줄여 넣을 때마다 지우지 않게 한다.
        let target = self.cap_bytes / 10 * 9;
        let mut excess = total.saturating_sub(target) as i64;
        let victims: Vec<(String, i64)> = {
            let db = self.lock();
            let mut stmt = db.prepare("SELECT key, size FROM images ORDER BY last_access ASC")?;
            let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
            let mut out = Vec::new();
            for row in rows {
                let (key, size) = row?;
                if excess <= 0 {
                    break;
                }
                excess -= size;
                out.push((key, size));
            }
            out
        };
        for (key, _) in &victims {
            let _ = fs::remove_file(self.blob_path(key));
            self.lock()
                .execute("DELETE FROM images WHERE key = ?1", params![key])?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta() -> ImageMeta {
        ImageMeta {
            content_type: "image/jpeg".into(),
            etag: None,
            last_modified: None,
        }
    }

    #[test]
    fn put_then_get_round_trips_and_survives_reopen() {
        let dir = tempfile::tempdir().unwrap();
        {
            let c = ImageCache::open(dir.path(), DEFAULT_CAP_BYTES).unwrap();
            let m = ImageMeta {
                content_type: "image/png".into(),
                etag: Some("\"e1\"".into()),
                last_modified: None,
            };
            c.put("https://a/1.png", ImageKind::Avatar, &m, b"abc", 1_000)
                .unwrap();
        }
        let c = ImageCache::open(dir.path(), DEFAULT_CAP_BYTES).unwrap();
        let got = c.get("https://a/1.png", 2_000).unwrap().unwrap();
        assert_eq!(got.bytes, b"abc");
        assert_eq!(got.content_type, "image/png");
        assert_eq!(got.kind, ImageKind::Avatar);
        assert_eq!(got.etag.as_deref(), Some("\"e1\""));
        assert_eq!(c.stats().unwrap().entries, 1);
        assert!(c.get("https://a/other.png", 2_000).unwrap().is_none());
    }

    #[test]
    fn freshness_follows_kind_and_mark_fresh() {
        let dir = tempfile::tempdir().unwrap();
        let c = ImageCache::open(dir.path(), DEFAULT_CAP_BYTES).unwrap();
        c.put("https://l/t.jpg", ImageKind::Live, &meta(), b"x", 0)
            .unwrap();
        c.put("https://a/p.png", ImageKind::Avatar, &meta(), b"y", 0)
            .unwrap();
        let live = c.get("https://l/t.jpg", 44_000).unwrap().unwrap();
        assert!(live.is_fresh(44_000));
        assert!(!live.is_fresh(46_000), "라이브는 45초면 오래된 것");
        let avatar = c.get("https://a/p.png", 46_000).unwrap().unwrap();
        assert!(avatar.is_fresh(6 * 24 * 3_600_000));
        assert!(!avatar.is_fresh(8 * 24 * 3_600_000));
        c.mark_fresh("https://l/t.jpg", 100_000).unwrap();
        assert!(c
            .get("https://l/t.jpg", 120_000)
            .unwrap()
            .unwrap()
            .is_fresh(120_000));
    }

    #[test]
    fn evicts_least_recently_used_over_cap() {
        let dir = tempfile::tempdir().unwrap();
        let c = ImageCache::open(dir.path(), 1_000).unwrap();
        let blob = vec![7u8; 300];
        c.put("u1", ImageKind::Static, &meta(), &blob, 1).unwrap();
        c.put("u2", ImageKind::Static, &meta(), &blob, 2).unwrap();
        c.put("u3", ImageKind::Static, &meta(), &blob, 3).unwrap();
        // u1 을 최근에 썼다 → 넘치면 u2 가 먼저 나간다.
        assert!(c.get("u1", 4).unwrap().is_some());
        c.put("u4", ImageKind::Static, &meta(), &blob, 5).unwrap();
        let s = c.stats().unwrap();
        assert!(s.bytes <= 1_000, "상한 안: {}", s.bytes);
        assert!(
            c.get("u2", 6).unwrap().is_none(),
            "가장 오래 안 쓴 것이 나간다"
        );
        assert!(c.get("u1", 6).unwrap().is_some());
        assert!(c.get("u4", 6).unwrap().is_some());
    }

    #[test]
    fn clear_removes_everything() {
        let dir = tempfile::tempdir().unwrap();
        let c = ImageCache::open(dir.path(), DEFAULT_CAP_BYTES).unwrap();
        c.put("u1", ImageKind::Static, &meta(), b"abc", 1).unwrap();
        c.clear().unwrap();
        assert_eq!(c.stats().unwrap().entries, 0);
        assert!(c.get("u1", 2).unwrap().is_none());
        // 비운 뒤에도 다시 쓸 수 있다.
        c.put("u1", ImageKind::Static, &meta(), b"abc", 3).unwrap();
        assert!(c.get("u1", 4).unwrap().is_some());
    }

    #[test]
    fn missing_blob_is_a_miss_and_cleans_index() {
        let dir = tempfile::tempdir().unwrap();
        let c = ImageCache::open(dir.path(), DEFAULT_CAP_BYTES).unwrap();
        c.put("u1", ImageKind::Static, &meta(), b"abc", 1).unwrap();
        fs::remove_file(c.blob_path(&ImageCache::key("u1"))).unwrap();
        assert!(c.get("u1", 2).unwrap().is_none());
        assert_eq!(c.stats().unwrap().entries, 0);
    }

    #[test]
    fn oversized_or_empty_bodies_are_not_stored() {
        let dir = tempfile::tempdir().unwrap();
        let c = ImageCache::open(dir.path(), DEFAULT_CAP_BYTES).unwrap();
        c.put("empty", ImageKind::Static, &meta(), b"", 1).unwrap();
        c.put(
            "huge",
            ImageKind::Static,
            &meta(),
            &vec![0u8; MAX_ENTRY_BYTES + 1],
            1,
        )
        .unwrap();
        assert_eq!(c.stats().unwrap().entries, 0);
    }

    #[test]
    fn corrupt_index_is_rebuilt() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("index.sqlite"), b"not a database at all").unwrap();
        let c = ImageCache::open(dir.path(), DEFAULT_CAP_BYTES).unwrap();
        c.put("u1", ImageKind::Static, &meta(), b"abc", 1).unwrap();
        assert!(c.get("u1", 2).unwrap().is_some());
    }

    #[test]
    fn kind_parsing() {
        assert_eq!(ImageKind::parse("hero"), ImageKind::Live);
        assert_eq!(ImageKind::parse("avatar-lg"), ImageKind::Avatar);
        assert_eq!(ImageKind::parse("poster"), ImageKind::Static);
    }
}
