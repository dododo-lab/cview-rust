//! 홈 추천 채널 점수.
//!
//! 원본: `Sources/CViewApp/ViewModels/HomeRecommendationEngine.swift`.

use std::collections::HashSet;

use serde::Serialize;

use cview_core::models::LiveChannelItem;

/// 점수 매긴 채널.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScoredChannel {
    pub channel: LiveChannelItem,
    pub score: f64,
    pub reasons: Vec<String>,
}

impl ScoredChannel {
    /// `Identifiable.id` — channelId.
    #[allow(clippy::misnamed_getters)]
    pub fn id(&self) -> &str {
        &self.channel.channel_id
    }
}

/// 점수 입력.
#[derive(Debug, Clone, Default)]
pub struct RecommendationInputs {
    pub candidates: Vec<LiveChannelItem>,
    pub following_channel_ids: HashSet<String>,
    pub favorite_channel_ids: HashSet<String>,
    pub recent_channel_ids: HashSet<String>,
    pub recent_categories: HashSet<String>,
    pub already_watching_channel_ids: HashSet<String>,
}

pub const DEFAULT_LIMIT: usize = 12;

/// 팔로잉 100 · 즐겨찾기 80 · 최근 시청 60 · 관심 카테고리 25 · 시청자 비율 ×15, 이미 보는 채널 −100.
/// 0 이하는 버리고 점수 내림차순 상위 `limit`.
pub fn score(inputs: &RecommendationInputs, limit: usize) -> Vec<ScoredChannel> {
    if inputs.candidates.is_empty() {
        return Vec::new();
    }
    let max_viewer = inputs
        .candidates
        .iter()
        .map(|c| c.viewer_count)
        .max()
        .unwrap_or(1)
        .max(1);
    let mut results: Vec<ScoredChannel> = inputs
        .candidates
        .iter()
        .map(|ch| {
            let mut score = 0.0;
            let mut reasons = Vec::new();
            if inputs.following_channel_ids.contains(&ch.channel_id) {
                score += 100.0;
                reasons.push("팔로잉".to_string());
            }
            if inputs.favorite_channel_ids.contains(&ch.channel_id) {
                score += 80.0;
                reasons.push("즐겨찾기".to_string());
            }
            if inputs.recent_channel_ids.contains(&ch.channel_id) {
                score += 60.0;
                reasons.push("최근 시청".to_string());
            }
            if let Some(cat) = ch.category_name.as_deref() {
                if !cat.is_empty() && inputs.recent_categories.contains(cat) {
                    score += 25.0;
                    reasons.push("관심 카테고리".to_string());
                }
            }
            score += ch.viewer_count as f64 / max_viewer as f64 * 15.0;
            if inputs.already_watching_channel_ids.contains(&ch.channel_id) {
                score -= 100.0;
            }
            ScoredChannel {
                channel: ch.clone(),
                score,
                reasons,
            }
        })
        .filter(|s| s.score > 0.0)
        .collect();
    // Swift `sorted` 는 안정 정렬이 아니지만 같은 점수의 상대 순서는 계약이 아니다 — 안정 정렬로 둔다.
    results.sort_by(|a, b| b.score.total_cmp(&a.score));
    results.truncate(limit);
    results
}
