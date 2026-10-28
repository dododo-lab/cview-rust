//! 줍(zap) 계측 — 채널 전환 요청부터 첫 화면까지의 시간을 기록한다.

use tokio::time::Instant;

use super::{ManagerState, ZapRequest};
use crate::viewmodels::multi_live::policy::{ZAP_REQUEST_CAP, ZAP_REQUEST_MAX_AGE_SECS};

impl ManagerState {
    pub fn note_zap_requested(&mut self, channel_id: &str) {
        let now = Instant::now();
        if let Some(existing) = self.zap_requests.get(channel_id) {
            if now.duration_since(existing.at).as_secs_f64() <= ZAP_REQUEST_MAX_AGE_SECS {
                return;
            }
        }
        let warm = self.session_for_channel(channel_id).is_some();
        self.zap_requests
            .insert(channel_id.to_string(), ZapRequest { at: now, warm });
        if self.zap_requests.len() > ZAP_REQUEST_CAP {
            if let Some(oldest) = self
                .zap_requests
                .iter()
                .min_by_key(|(_, r)| r.at)
                .map(|(k, _)| k.clone())
            {
                self.zap_requests.remove(&oldest);
            }
        }
    }

    pub fn forget_zap_request(&mut self, channel_id: &str) {
        self.zap_requests.remove(channel_id);
    }

    pub fn has_zap_request(&self, channel_id: &str) -> bool {
        self.zap_requests.contains_key(channel_id)
    }

    pub fn complete_watch_zap(&mut self, channel_id: &str, reason: &str) {
        let now = Instant::now();
        let Some(request) = self.zap_requests.remove(channel_id) else {
            return;
        };
        let elapsed = now.duration_since(request.at).as_secs_f64();
        if elapsed > ZAP_REQUEST_MAX_AGE_SECS {
            return;
        }
        tracing::info!(
            "live_zap phase={} reason={reason} total_ms={} sessions={}",
            if request.warm { "warm" } else { "cold" },
            elapsed * 1_000.0,
            self.sessions.len()
        );
    }
}
