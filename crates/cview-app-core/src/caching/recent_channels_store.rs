//! `Sources/CViewApp/Caching/RecentChannelsStore.swift` 포팅.
//!
//! 최근 시청 채널의 메모리 미러. 홈 레일(24개)과 라이브 개요(30개)가 같은 원천을 쓴다.
//! 채우기는 부팅 때 한 번(`hydrate`), 시청 시작은 `note_watch` 가 시청 시각 자리에 넣는다.
//! 이긴 행에 아바타가 없으면 진 행의 아바타를 이어받고, `clear()` 세대를 보고 늦은 도착을 버린다.

use std::collections::HashMap;

use cview_core::utils::time::Date;
use cview_persistence::models::persisted_channel::ChannelListData;

#[derive(Debug, Clone, Default)]
pub struct RecentChannelsStore {
    items: Vec<ChannelListData>,
    did_hydrate: bool,
    /// 비우기와 늦게 도착한 채우기를 구분한다. 지우기가 오면 올라간다.
    generation: u64,
}

impl RecentChannelsStore {
    pub const CAPACITY: usize = 30;
    pub const HOME_LIMIT: usize = 24;

    pub fn new() -> Self {
        Self::default()
    }

    pub fn items(&self) -> &[ChannelListData] {
        &self.items
    }

    pub fn did_hydrate(&self) -> bool {
        self.did_hydrate
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn channel_ids(&self) -> Vec<String> {
        self.items.iter().map(|i| i.channel_id.clone()).collect()
    }

    pub fn home_items(&self) -> Vec<ChannelListData> {
        self.items.iter().take(Self::HOME_LIMIT).cloned().collect()
    }

    /// 배열이 실제로 바뀌었을 때만 대입한다. 바뀌었는지 돌려준다.
    fn assign(&mut self, next: Vec<ChannelListData>) -> bool {
        if next != self.items {
            self.items = next;
            true
        } else {
            false
        }
    }

    pub fn hydrate(&mut self, generation: u64, fetched: &[ChannelListData]) -> bool {
        if generation != self.generation {
            return false;
        }
        let next = Self::merge(fetched, &self.items);
        let changed = self.assign(next);
        self.did_hydrate = true;
        changed
    }

    /// 영속 `startWatchRecord` 가 성공한 뒤에만 부른다.
    ///
    /// `image_url` 은 채널 아바타 — 라이브 프레임(thumbnail)은 넘기지 않는다.
    /// `generation` 은 호출부가 기록 **전에** 잡은 값이다.
    pub fn note_watch(
        &mut self,
        channel_id: &str,
        channel_name: &str,
        image_url: Option<&str>,
        is_favorite: bool,
        watched_at: Date,
        generation: u64,
    ) -> bool {
        if generation != self.generation {
            return false;
        }
        let existing = self.items.iter().find(|i| i.channel_id == channel_id);
        let name = if channel_name.is_empty() {
            existing.map(|e| e.channel_name.clone()).unwrap_or_default()
        } else {
            channel_name.to_string()
        };
        let avatar = image_url
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .or_else(|| existing.and_then(|e| e.image_url.clone()));
        let row = ChannelListData {
            channel_id: channel_id.to_string(),
            channel_name: name,
            image_url: avatar,
            is_favorite,
            last_watched: Some(watched_at),
        };
        let next = Self::merge(&self.items, &[row]);
        self.assign(next)
    }

    pub fn apply_favorite(&mut self, channel_id: &str, is_favorite: bool) -> bool {
        let next: Vec<ChannelListData> = self
            .items
            .iter()
            .map(|c| {
                if c.channel_id == channel_id && c.is_favorite != is_favorite {
                    ChannelListData {
                        is_favorite,
                        ..c.clone()
                    }
                } else {
                    c.clone()
                }
            })
            .collect();
        self.assign(next)
    }

    pub fn clear(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.items.clear();
        self.did_hydrate = true;
    }

    /// 영속 목록이 바닥이고, 메모리에만 있는 더 최근 시청을 덮지 않는다.
    pub fn merge(
        persisted: &[ChannelListData],
        overlay: &[ChannelListData],
    ) -> Vec<ChannelListData> {
        let mut by_id: HashMap<String, ChannelListData> = HashMap::new();
        for row in persisted.iter().chain(overlay) {
            let merged = Self::newer(by_id.remove(&row.channel_id), row.clone());
            by_id.insert(row.channel_id.clone(), merged);
        }
        // 같은 시각은 채널 ID 로 순서를 고정한다.
        let mut rows: Vec<ChannelListData> = by_id.into_values().collect();
        rows.sort_by(|l, r| {
            let la = l.last_watched.unwrap_or(Date::MIN_UTC);
            let ra = r.last_watched.unwrap_or(Date::MIN_UTC);
            ra.cmp(&la).then_with(|| l.channel_id.cmp(&r.channel_id))
        });
        rows.truncate(Self::CAPACITY);
        rows
    }

    /// 더 새 시각의 행을 고른다. 이긴 행에 아바타가 없고 진 행에 있으면 진 행의 아바타를 옮긴다.
    fn newer(current: Option<ChannelListData>, candidate: ChannelListData) -> ChannelListData {
        let Some(current) = current else {
            return candidate;
        };
        let current_at = current.last_watched.unwrap_or(Date::MIN_UTC);
        let candidate_at = candidate.last_watched.unwrap_or(Date::MIN_UTC);
        let (mut winner, loser) = if candidate_at >= current_at {
            (candidate, current)
        } else {
            (current, candidate)
        };
        let winner_empty = winner.image_url.as_deref().is_none_or(str::is_empty);
        if winner_empty {
            if let Some(avatar) = loser.image_url.filter(|a| !a.is_empty()) {
                winner.image_url = Some(avatar);
            }
        }
        winner
    }
}

#[cfg(test)]
mod tests {
    //! 원본: `Tests/CViewAppTests/RecentChannelsStoreTests.swift` (호출부 소스 가드 제외).
    use super::*;
    use cview_core::utils::time::{adding, from_unix};

    fn base() -> Date {
        from_unix(1_700_000_000.0)
    }

    fn at(secs: f64) -> Date {
        adding(&base(), secs)
    }

    fn row(id: &str, minutes_ago: f64) -> ChannelListData {
        ChannelListData {
            channel_id: id.into(),
            channel_name: id.into(),
            image_url: Some(format!("https://example.invalid/{id}.png")),
            is_favorite: false,
            last_watched: Some(adding(&base(), -minutes_ago * 60.0)),
        }
    }

    #[test]
    fn hydrate_caps_at_thirty_newest() {
        let mut s = RecentChannelsStore::new();
        let rows: Vec<_> = (0..40).map(|i| row(&format!("c{i}"), i as f64)).collect();
        s.hydrate(s.generation(), &rows);
        assert_eq!(s.items().len(), 30);
        assert_eq!(s.items()[0].channel_id, "c0");
        assert_eq!(s.items()[29].channel_id, "c29");
        assert_eq!(s.home_items().len(), 24);
        assert!(s.did_hydrate());
    }

    #[test]
    fn same_hydrate_keeps_the_array() {
        let mut s = RecentChannelsStore::new();
        let rows = [row("a", 1.0)];
        assert!(s.hydrate(s.generation(), &rows));
        assert!(!s.hydrate(s.generation(), &rows));
    }

    #[test]
    fn watch_before_hydrate_wins_when_newer() {
        let mut s = RecentChannelsStore::new();
        s.note_watch("live", "라이브", None, true, at(1.0), s.generation());
        s.hydrate(s.generation(), &[row("old", 10.0), row("live", 5.0)]);
        let first = &s.items()[0];
        assert_eq!(first.channel_id, "live");
        assert_eq!(first.last_watched, Some(at(1.0)));
        assert_eq!(first.channel_name, "라이브");
        assert!(first.is_favorite);
        assert_eq!(
            first.image_url.as_deref(),
            Some("https://example.invalid/live.png")
        );
        assert!(s.channel_ids().contains(&"old".to_string()));
    }

    #[test]
    fn note_watch_moves_to_front_and_keeps_avatar() {
        let mut s = RecentChannelsStore::new();
        let rows: Vec<_> = (0..30)
            .map(|i| row(&format!("c{i}"), (i + 1) as f64))
            .collect();
        s.hydrate(s.generation(), &rows);
        let avatar = s.items()[10].image_url.clone();
        s.note_watch("c10", "c10", None, false, at(1.0), s.generation());
        assert_eq!(s.items()[0].channel_id, "c10");
        assert_eq!(s.items()[0].image_url, avatar);
        assert_eq!(s.items().len(), 30);
        s.note_watch("fresh", "fresh", None, false, at(2.0), s.generation());
        assert_eq!(s.items()[0].channel_id, "fresh");
        assert_eq!(s.items().len(), 30);
        assert!(!s.channel_ids().contains(&"c29".to_string()));
    }

    #[test]
    fn first_time_channel_keeps_given_avatar() {
        let mut s = RecentChannelsStore::new();
        let rows: Vec<_> = (0..30)
            .map(|i| row(&format!("c{i}"), (i + 1) as f64))
            .collect();
        s.hydrate(s.generation(), &rows);
        let new = "https://example.invalid/new.png";
        s.note_watch("fresh", "fresh", Some(new), false, at(1.0), s.generation());
        assert_eq!(s.items()[0].image_url.as_deref(), Some(new));
        s.note_watch("fresh", "fresh", None, false, at(2.0), s.generation());
        assert_eq!(s.items()[0].last_watched, Some(at(2.0)));
        assert_eq!(s.items()[0].image_url.as_deref(), Some(new));
    }

    #[test]
    fn empty_avatar_keeps_existing_and_non_empty_replaces() {
        let mut s = RecentChannelsStore::new();
        s.hydrate(s.generation(), &[row("a", 1.0)]);
        s.note_watch("a", "a", None, false, at(1.0), s.generation());
        assert_eq!(
            s.items()[0].image_url.as_deref(),
            Some("https://example.invalid/a.png")
        );
        s.note_watch("a", "a", Some(""), false, at(2.0), s.generation());
        assert_eq!(
            s.items()[0].image_url.as_deref(),
            Some("https://example.invalid/a.png")
        );
        s.note_watch(
            "a",
            "a",
            Some("https://example.invalid/a2.png"),
            false,
            at(3.0),
            s.generation(),
        );
        assert_eq!(
            s.items()[0].image_url.as_deref(),
            Some("https://example.invalid/a2.png")
        );
        assert_eq!(s.items().len(), 1);
    }

    #[test]
    fn merge_carries_avatar_from_older_row() {
        let persisted = row("a", 5.0);
        let bare = ChannelListData {
            channel_id: "a".into(),
            channel_name: "a-new".into(),
            image_url: None,
            is_favorite: true,
            last_watched: Some(at(1.0)),
        };
        let merged = RecentChannelsStore::merge(std::slice::from_ref(&persisted), &[bare]);
        assert_eq!(merged.len(), 1);
        assert_eq!(
            merged[0].image_url.as_deref(),
            Some("https://example.invalid/a.png")
        );
        assert_eq!(merged[0].channel_name, "a-new");
        assert!(merged[0].is_favorite);

        let newer_bare = ChannelListData {
            channel_id: "b".into(),
            channel_name: "b".into(),
            image_url: Some(String::new()),
            is_favorite: false,
            last_watched: Some(at(2.0)),
        };
        let reversed = RecentChannelsStore::merge(&[newer_bare], &[row("b", 30.0)]);
        assert_eq!(reversed[0].last_watched, Some(at(2.0)));
        assert_eq!(
            reversed[0].image_url.as_deref(),
            Some("https://example.invalid/b.png")
        );

        let own = ChannelListData {
            channel_id: "a".into(),
            channel_name: "a".into(),
            image_url: Some("https://example.invalid/own.png".into()),
            is_favorite: false,
            last_watched: Some(at(1.0)),
        };
        let kept = RecentChannelsStore::merge(&[persisted], &[own]);
        assert_eq!(
            kept[0].image_url.as_deref(),
            Some("https://example.invalid/own.png")
        );
    }

    #[test]
    fn older_watch_lands_in_time_order() {
        let mut s = RecentChannelsStore::new();
        s.note_watch("b", "b", None, false, at(20.0), s.generation());
        s.note_watch("a", "a", None, false, at(10.0), s.generation());
        assert_eq!(s.channel_ids(), vec!["b", "a"]);
        s.note_watch("b", "b", None, false, at(5.0), s.generation());
        assert_eq!(s.channel_ids(), vec!["b", "a"]);
        assert_eq!(s.items()[0].last_watched, Some(at(20.0)));
    }

    #[test]
    fn unchanged_note_watch_does_not_reassign() {
        let mut s = RecentChannelsStore::new();
        s.hydrate(s.generation(), &[row("a", 1.0), row("b", 2.0)]);
        s.note_watch("a", "a", None, false, at(1.0), s.generation());
        assert!(!s.note_watch("a", "a", None, false, at(1.0), s.generation()));
        assert!(s.note_watch("b", "b", None, false, at(2.0), s.generation()));
        assert_eq!(s.channel_ids(), vec!["b", "a"]);
    }

    #[test]
    fn clear_drops_late_hydrate_and_note_watch() {
        let mut s = RecentChannelsStore::new();
        let g = s.generation();
        s.clear();
        s.hydrate(g, &[row("stale", 1.0)]);
        assert!(s.items().is_empty());

        s.hydrate(s.generation(), &[row("a", 1.0)]);
        let g = s.generation();
        s.clear();
        s.note_watch("a", "a", None, false, at(1.0), g);
        assert!(s.items().is_empty());
        s.note_watch("a", "a", None, false, at(2.0), s.generation());
        assert_eq!(s.channel_ids(), vec!["a"]);
    }
}
