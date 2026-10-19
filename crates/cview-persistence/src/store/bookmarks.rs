//! 스트림 북마크.
//!
//! 원본: `Sources/CViewPersistence/DataStore+Bookmarks.swift`.

use super::DataStore;
use crate::db;
use crate::error::Result;
use crate::models::{StreamBookmark, StreamBookmarkData};
use crate::revision::WatchInsightsRevision;
use crate::time::{epoch_seconds, now, swift_double_description, TimeInterval};
use rusqlite::types::Value;

/// 런 간 안정적인 북마크 식별자 — `"channelId|<capturedAt.timeIntervalSince1970>"`.
fn bookmark_key(b: &StreamBookmark) -> String {
    format!(
        "{}|{}",
        b.channel_id,
        swift_double_description(epoch_seconds(&b.captured_at))
    )
}

fn to_data(b: &StreamBookmark) -> StreamBookmarkData {
    StreamBookmarkData {
        id: bookmark_key(b),
        channel_id: b.channel_id.clone(),
        channel_name: b.channel_name.clone(),
        live_title: b.live_title.clone(),
        category_name: b.category_name.clone(),
        thumbnail_url: b.thumbnail_url.clone(),
        captured_at: b.captured_at,
        live_uptime: b.live_uptime,
        note: b.note.clone(),
    }
}

impl DataStore {
    /// 새 북마크 저장(`capturedAt = now`). 성공하면 인사이트 리비전을 올린다.
    #[allow(clippy::too_many_arguments)]
    pub fn save_bookmark(
        &self,
        channel_id: &str,
        channel_name: &str,
        live_title: &str,
        category_name: Option<&str>,
        thumbnail_url: Option<&str>,
        live_uptime: TimeInterval,
        note: Option<&str>,
    ) -> Result<StreamBookmarkData> {
        let bookmark = self.write(|tx| {
            let mut bookmark = StreamBookmark {
                id: 0,
                channel_id: channel_id.into(),
                channel_name: channel_name.into(),
                live_title: live_title.into(),
                category_name: category_name.map(str::to_string),
                thumbnail_url: thumbnail_url.map(str::to_string),
                captured_at: now(),
                live_uptime,
                note: note.map(str::to_string),
            };
            db::insert(tx, &mut bookmark)?;
            Ok((bookmark, true))
        })?;
        WatchInsightsRevision::bump();
        Ok(to_data(&bookmark))
    }

    /// 북마크 삭제. 없는 id 면 아무것도 하지 않는다(리비전도 그대로).
    pub fn delete_bookmark(&self, id: &str) -> Result<()> {
        let deleted = self.write(|tx| {
            for bookmark in db::fetch::<StreamBookmark>(tx, "", &[])? {
                if bookmark_key(&bookmark) == id {
                    db::delete(tx, &bookmark)?;
                    return Ok((true, true));
                }
            }
            Ok((false, false))
        })?;
        if deleted {
            WatchInsightsRevision::bump();
        }
        Ok(())
    }

    /// 모든 북마크(최신순, limit 개).
    pub fn fetch_bookmarks(&self, limit: usize) -> Result<Vec<StreamBookmarkData>> {
        self.read(|conn| {
            Ok(db::fetch::<StreamBookmark>(
                conn,
                "ORDER BY capturedAt DESC, id DESC LIMIT ?",
                &[Value::Integer(limit as i64)],
            )?
            .iter()
            .map(to_data)
            .collect())
        })
    }

    /// 특정 채널의 북마크(최신순).
    pub fn fetch_bookmarks_for_channel(&self, channel_id: &str) -> Result<Vec<StreamBookmarkData>> {
        self.read(|conn| {
            Ok(db::fetch::<StreamBookmark>(
                conn,
                "WHERE channelId = ? ORDER BY capturedAt DESC, id DESC",
                &[Value::Text(channel_id.into())],
            )?
            .iter()
            .map(to_data)
            .collect())
        })
    }

    /// 북마크 총 개수.
    pub fn bookmark_count(&self) -> Result<i64> {
        self.read(|conn| Ok(db::count::<StreamBookmark>(conn, "", &[])?))
    }
}
