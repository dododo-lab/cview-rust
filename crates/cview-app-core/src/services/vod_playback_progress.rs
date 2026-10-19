//! `Sources/CViewApp/Services/VODPlaybackProgressStore.swift` 포팅.
//!
//! VOD 카드의 이어 보기 진행률 막대가 읽는 메모리 미러. 원천은 영속 `PersistedVODPlayback` 이다.
//! 스토어가 열리면 한 번 채우고(`hydrate`), 플레이어의 기록·삭제가 같은 값을 반영한다.
//!
//! 플레이어 쪽 스냅샷 타입(`VODProgressPersistenceSnapshot`, `VODPlayerViewModel.swift`)은 뷰모델
//! 몫이라, 여기서는 그 판정 규칙만 [`VodProgressDisposition::classify`] 로 옮겼다.

use std::collections::{HashMap, HashSet};

use cview_core::utils::time::TimeInterval;
use cview_persistence::models::playback::PersistedVodPlaybackData;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VodPlaybackProgress {
    pub video_no: i64,
    pub position: TimeInterval,
    pub duration: TimeInterval,
}

impl VodPlaybackProgress {
    pub fn fraction(&self) -> f64 {
        if self.duration <= 0.0 {
            return 0.0;
        }
        (self.position / self.duration).clamp(0.0, 1.0)
    }
}

/// 이어 보기 영속화 판정(기록 = 값, 삭제 = 제거, 무시 = 그대로).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VodProgressDisposition {
    Ignore,
    Record,
    Remove,
}

impl VodProgressDisposition {
    /// 길이가 없으면 무시, 끝났거나 95% 이상이면 삭제, 30초 넘게 봤으면 기록.
    pub fn classify(position: TimeInterval, duration: TimeInterval, playback_ended: bool) -> Self {
        if duration <= 0.0 {
            return Self::Ignore;
        }
        if playback_ended || position / duration >= 0.95 {
            return Self::Remove;
        }
        if position > 30.0 {
            Self::Record
        } else {
            Self::Ignore
        }
    }
}

#[derive(Debug, Default)]
pub struct VodPlaybackProgressStore {
    entries: HashMap<i64, VodPlaybackProgress>,
    is_hydrated: bool,
    removed_before_hydrate: HashSet<i64>,
}

impl VodPlaybackProgressStore {
    /// 스토어에서 채울 최대 행 수.
    pub const HYDRATE_LIMIT: usize = 200;

    pub fn new() -> Self {
        Self::default()
    }

    pub fn entries(&self) -> &HashMap<i64, VodPlaybackProgress> {
        &self.entries
    }

    pub fn is_hydrated(&self) -> bool {
        self.is_hydrated
    }

    pub fn progress(&self, video_no: i64) -> Option<VodPlaybackProgress> {
        self.entries.get(&video_no).copied()
    }

    /// 영속 이어 보기 기록으로 채운다. 이미 채웠으면 아무것도 하지 않는다.
    /// 채우는 동안 플레이어가 기록·삭제한 값이 더 새롭다.
    pub fn hydrate(&mut self, rows: &[PersistedVodPlaybackData]) {
        if self.is_hydrated {
            return;
        }
        self.is_hydrated = true;
        let mut merged: HashMap<i64, VodPlaybackProgress> = HashMap::new();
        for row in rows {
            merged.entry(row.video_no).or_insert(VodPlaybackProgress {
                video_no: row.video_no,
                position: row.last_position_sec,
                duration: row.duration_sec,
            });
        }
        for (video_no, entry) in &self.entries {
            merged.insert(*video_no, *entry);
        }
        for video_no in self.removed_before_hydrate.drain() {
            merged.remove(&video_no);
        }
        if merged != self.entries {
            self.entries = merged;
        }
    }

    /// 플레이어가 영속화할 값을 반영한다.
    pub fn apply(
        &mut self,
        video_no: i64,
        position: TimeInterval,
        duration: TimeInterval,
        disposition: VodProgressDisposition,
    ) {
        match disposition {
            VodProgressDisposition::Ignore => {}
            VodProgressDisposition::Record => {
                self.entries.insert(
                    video_no,
                    VodPlaybackProgress {
                        video_no,
                        position,
                        duration,
                    },
                );
            }
            VodProgressDisposition::Remove => self.clear(video_no),
        }
    }

    pub fn clear(&mut self, video_no: i64) {
        if !self.is_hydrated {
            self.removed_before_hydrate.insert(video_no);
        }
        self.entries.remove(&video_no);
    }

    /// 로컬 데이터 삭제·로그아웃 정리.
    pub fn remove_all(&mut self) {
        self.entries.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cview_core::utils::time::from_unix;

    fn row(video_no: i64, pos: f64) -> PersistedVodPlaybackData {
        PersistedVodPlaybackData {
            video_no,
            last_position_sec: pos,
            duration_sec: 100.0,
            video_title: String::new(),
            channel_name: String::new(),
            thumbnail_url_string: None,
            last_seen_at: from_unix(0.0),
        }
    }

    #[test]
    fn player_values_before_hydrate_win() {
        let mut s = VodPlaybackProgressStore::new();
        s.apply(1, 50.0, 100.0, VodProgressDisposition::Record);
        s.clear(2);
        s.hydrate(&[row(1, 40.0), row(2, 40.0), row(3, 40.0)]);
        assert_eq!(s.progress(1).unwrap().position, 50.0);
        assert!(s.progress(2).is_none());
        assert!((s.progress(3).unwrap().fraction() - 0.4).abs() < 1e-9);
        s.hydrate(&[row(9, 1.0)]);
        assert!(s.progress(9).is_none(), "두 번 채우지 않는다");
    }

    #[test]
    fn disposition_rules() {
        assert_eq!(
            VodProgressDisposition::classify(10.0, 0.0, false),
            VodProgressDisposition::Ignore
        );
        assert_eq!(
            VodProgressDisposition::classify(95.0, 100.0, false),
            VodProgressDisposition::Remove
        );
        assert_eq!(
            VodProgressDisposition::classify(31.0, 100.0, false),
            VodProgressDisposition::Record
        );
        assert_eq!(
            VodProgressDisposition::classify(30.0, 100.0, false),
            VodProgressDisposition::Ignore
        );
        assert_eq!(
            VodProgressDisposition::classify(1.0, 100.0, true),
            VodProgressDisposition::Remove
        );
    }
}
