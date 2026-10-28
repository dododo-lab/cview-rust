//! 클립 브라우저 — 나중에 볼 클립·재생 대기열·최근 본 클립.

use std::collections::HashMap;

use cview_core::models::search::ClipInfo;
use cview_core::models::ChannelInfo;
use serde::Serialize;

use super::{
    ClipBrowserViewModel, MAX_RECENT_CHANNELS, QUEUE_DEFAULTS_KEY, RECENT_CHANNELS_DEFAULTS_KEY,
    SAVED_SNAPSHOTS_DEFAULTS_KEY, WATCH_LATER_DEFAULTS_KEY,
};

impl ClipBrowserViewModel {
    /// 저장소에서 나중에 보기·스냅샷·큐·최근 채널을 읽는다.
    pub fn load_watch_later_from_defaults(&self) {
        let d = self.shared.defaults.as_ref();
        let mut st = self.lock();
        if let Some(arr) = d
            .string(WATCH_LATER_DEFAULTS_KEY)
            .and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
        {
            st.watch_later_uids = arr.into_iter().collect();
        }
        if let Some(m) = d
            .string(SAVED_SNAPSHOTS_DEFAULTS_KEY)
            .and_then(|s| serde_json::from_str::<HashMap<String, ClipInfo>>(&s).ok())
        {
            st.saved_clip_snapshots = m;
        }
        if let Some(q) = d
            .string(QUEUE_DEFAULTS_KEY)
            .and_then(|s| serde_json::from_str::<Vec<ClipInfo>>(&s).ok())
        {
            st.queue_clips = q;
        }
        if let Some(r) = d
            .string(RECENT_CHANNELS_DEFAULTS_KEY)
            .and_then(|s| serde_json::from_str::<Vec<ChannelInfo>>(&s).ok())
        {
            st.recent_channels = r;
        }
    }

    fn persist<T: Serialize>(&self, key: &str, value: &T) {
        if let Ok(s) = serde_json::to_string(value) {
            self.shared.defaults.set_string(key, &s);
        }
    }

    /// 최근 채널 추가 — 중복 제거 후 맨 앞, 최대 8개.
    pub fn add_recent_channel(&self, channel: ChannelInfo) {
        let list = {
            let mut st = self.lock();
            st.recent_channels
                .retain(|c| c.channel_id != channel.channel_id);
            st.recent_channels.insert(0, channel);
            st.recent_channels.truncate(MAX_RECENT_CHANNELS);
            st.recent_channels.clone()
        };
        self.persist(RECENT_CHANNELS_DEFAULTS_KEY, &list);
    }

    pub fn clear_recent_channels(&self) {
        self.lock().recent_channels.clear();
        self.persist(RECENT_CHANNELS_DEFAULTS_KEY, &Vec::<ChannelInfo>::new());
    }

    pub fn toggle_watch_later(&self, clip: &ClipInfo) {
        let (uids, snaps) = {
            let mut st = self.lock();
            if st.watch_later_uids.remove(&clip.clip_uid) {
                st.saved_clip_snapshots.remove(&clip.clip_uid);
            } else {
                st.watch_later_uids.insert(clip.clip_uid.clone());
                st.saved_clip_snapshots
                    .insert(clip.clip_uid.clone(), clip.clone());
            }
            let mut uids: Vec<String> = st.watch_later_uids.iter().cloned().collect();
            uids.sort();
            (uids, st.saved_clip_snapshots.clone())
        };
        self.persist(WATCH_LATER_DEFAULTS_KEY, &uids);
        self.persist(SAVED_SNAPSHOTS_DEFAULTS_KEY, &snaps);
    }

    pub fn add_to_queue(&self, clip: &ClipInfo) {
        let q = {
            let mut st = self.lock();
            if st.is_in_queue(clip) {
                return;
            }
            st.queue_clips.push(clip.clone());
            st.queue_clips.clone()
        };
        self.persist(QUEUE_DEFAULTS_KEY, &q);
    }

    pub fn remove_from_queue(&self, clip: &ClipInfo) {
        let q = {
            let mut st = self.lock();
            st.queue_clips.retain(|c| c.clip_uid != clip.clip_uid);
            st.queue_clips.clone()
        };
        self.persist(QUEUE_DEFAULTS_KEY, &q);
    }

    pub fn toggle_queue(&self, clip: &ClipInfo) {
        let in_queue = self.read().is_in_queue(clip);
        if in_queue {
            self.remove_from_queue(clip);
        } else {
            self.add_to_queue(clip);
        }
    }

    pub fn clear_queue(&self) {
        self.lock().queue_clips.clear();
        self.persist(QUEUE_DEFAULTS_KEY, &Vec::<ClipInfo>::new());
    }
}
