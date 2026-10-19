//! VOD 메타 캐시와 계정 범위 구독 사본.
//!
//! 원본: `Sources/CViewPersistence/DataStore+VODCache.swift`
//! (`SubscriptionVODPage`, upsert/retain/fetch, `fetchSubscriptionVODs`, `applySubscriptionVODPages`,
//! `purgeSubscriptionVODTags`, `SubscriptionVODOrder`).

use super::DataStore;
use crate::db::{self, join_sources, split_sources};
use crate::error::Result;
use crate::models::{CachedVod, CachedVodData};
use crate::time::Date;
use rusqlite::types::Value;
use rusqlite::Connection;
use std::collections::{BTreeSet, HashMap, HashSet};

const VOD_CACHE_KEY_CHUNK_SIZE: usize = 200;

/// 한 채널의 구독 VOD 한 페이지(`SubscriptionVODPage`). 실패한 요청은 넘기지 않는다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubscriptionVodPage {
    pub channel_id: String,
    /// 0 부터. page 0 은 그 채널의 최신 쪽을 교체하고, 1 이상은 추가만 한다.
    pub page: i64,
    /// `channel_id` 가 없는 항목은 이 페이지의 채널로 채운다.
    pub items: Vec<CachedVodData>,
    /// 이 페이지가 마지막이다. page 0 이면 이번에 온 항목 밖의 행을 모두 뗀다.
    pub exhausted: bool,
}

impl SubscriptionVodPage {
    pub fn new(channel_id: impl Into<String>, page: i64, items: Vec<CachedVodData>) -> Self {
        Self {
            channel_id: channel_id.into(),
            page,
            items,
            exhausted: false,
        }
    }

    pub fn exhausted(mut self, exhausted: bool) -> Self {
        self.exhausted = exhausted;
        self
    }
}

/// 구독 피드 순서 — 발행일 최신, 발행일 없는 행은 뒤, 같으면 videoNo 큰 쪽.
pub fn subscription_is_newer(
    lhs_date: Option<Date>,
    lhs_no: i64,
    rhs_date: Option<Date>,
    rhs_no: i64,
) -> bool {
    match (lhs_date, rhs_date) {
        (Some(l), Some(r)) => {
            if l != r {
                l > r
            } else {
                lhs_no > rhs_no
            }
        }
        (Some(_), None) => true,
        (None, Some(_)) => false,
        (None, None) => lhs_no > rhs_no,
    }
}

fn newer_ordering(a: &CachedVod, b: &CachedVod) -> std::cmp::Ordering {
    if subscription_is_newer(a.publish_date, a.video_no, b.publish_date, b.video_no) {
        std::cmp::Ordering::Less
    } else if subscription_is_newer(b.publish_date, b.video_no, a.publish_date, a.video_no) {
        std::cmp::Ordering::Greater
    } else {
        std::cmp::Ordering::Equal
    }
}

/// 계정 토큰 형태인가 — 접두사 뒤에 scope 가 있고 쉼표가 없다.
pub(crate) fn is_scoped_subscription_tag(tag: &str) -> bool {
    tag.len() > CachedVod::SUBSCRIPTION_TAG_PREFIX.len()
        && tag.starts_with(CachedVod::SUBSCRIPTION_TAG_PREFIX)
        && !tag.contains(',')
}

fn has_tag(sources: &str, tag: &str) -> bool {
    sources.split(',').any(|t| t == tag)
}

fn make_row(item: &CachedVodData) -> CachedVod {
    CachedVod {
        id: 0,
        video_no: item.video_no,
        video_id: item.video_id.clone(),
        video_title: item.video_title.clone(),
        thumbnail_image_url: item.thumbnail_image_url.clone(),
        duration: item.duration,
        read_count: item.read_count,
        publish_date: item.publish_date,
        channel_id: item.channel_id.clone(),
        channel_name: item.channel_name.clone(),
        channel_image_url: item.channel_image_url.clone(),
        content_hash: item.content_hash.clone(),
        sources: join_sources(&item.sources),
        fetched_at: item.fetched_at,
    }
}

/// 기존 행에 내용과 source 를 합친다. 내용과 source 가 그대로면 false.
fn merge(item: &CachedVodData, row: &mut CachedVod) -> bool {
    let mut set = item.sources.clone();
    set.extend(split_sources(&row.sources));
    let merged = join_sources(&set);
    if row.content_hash == item.content_hash && row.sources == merged {
        return false;
    }
    if item.video_id.is_some() {
        row.video_id = item.video_id.clone();
    }
    row.video_title = item.video_title.clone();
    if item.thumbnail_image_url.is_some() {
        row.thumbnail_image_url = item.thumbnail_image_url.clone();
    }
    row.duration = item.duration;
    row.read_count = item.read_count;
    if item.publish_date.is_some() {
        row.publish_date = item.publish_date;
    }
    if item.channel_id.is_some() {
        row.channel_id = item.channel_id.clone();
    }
    if item.channel_name.is_some() {
        row.channel_name = item.channel_name.clone();
    }
    if item.channel_image_url.is_some() {
        row.channel_image_url = item.channel_image_url.clone();
    }
    row.content_hash = item.content_hash.clone();
    row.sources = merged;
    row.fetched_at = item.fetched_at;
    true
}

/// upsert 본문 — 커밋하지 않는다. 바뀐 videoNo 와 이번 묶음의 행(videoNo → 행)을 돌려준다.
fn upsert_rows(
    conn: &Connection,
    items: &[CachedVodData],
) -> Result<(HashSet<i64>, HashMap<i64, CachedVod>)> {
    if items.is_empty() {
        return Ok((HashSet::new(), HashMap::new()));
    }
    let keys: BTreeSet<i64> = items.iter().map(|i| i.video_no).collect();
    let key_values: Vec<Value> = keys.into_iter().map(Value::Integer).collect();
    let mut by_no: HashMap<i64, CachedVod> =
        db::fetch_in::<CachedVod>(conn, "videoNo", &key_values, VOD_CACHE_KEY_CHUNK_SIZE)?
            .into_iter()
            .map(|r| (r.video_no, r))
            .collect();
    let mut changed = HashSet::new();
    for item in items {
        if let Some(row) = by_no.get_mut(&item.video_no) {
            if merge(item, row) {
                db::update(conn, row)?;
                changed.insert(item.video_no);
            }
        } else {
            let mut row = make_row(item);
            db::insert(conn, &mut row)?;
            by_no.insert(item.video_no, row);
            changed.insert(item.video_no);
        }
    }
    Ok((changed, by_no))
}

/// `tag` 가 정확한 토큰으로 붙은 행.
fn subscription_rows(conn: &Connection, tag: &str) -> Result<Vec<CachedVod>> {
    Ok(db::fetch::<CachedVod>(
        conn,
        "WHERE instr(sources, ?) > 0 ORDER BY id ASC",
        &[Value::Text(tag.into())],
    )?
    .into_iter()
    .filter(|r| has_tag(&r.sources, tag))
    .collect())
}

/// channelId 로 묶고 채널 안을 피드 순서로 정렬한다. channelId 가 없는 행은 뺀다.
fn newest_first_by_channel<'a>(
    rows: impl IntoIterator<Item = &'a CachedVod>,
) -> HashMap<String, Vec<&'a CachedVod>> {
    let mut groups: HashMap<String, Vec<&CachedVod>> = HashMap::new();
    for row in rows {
        let Some(channel_id) = &row.channel_id else {
            continue;
        };
        groups.entry(channel_id.clone()).or_default().push(row);
    }
    for list in groups.values_mut() {
        list.sort_by(|a, b| newer_ordering(a, b));
    }
    groups
}

/// (b) page 0 이 온 채널에서 뗄 행.
fn stale_page_zero_rows(
    pages: &[SubscriptionVodPage],
    groups: &HashMap<String, Vec<&CachedVod>>,
) -> HashSet<i64> {
    let mut incoming: HashMap<&str, HashSet<i64>> = HashMap::new();
    for page in pages {
        incoming
            .entry(page.channel_id.as_str())
            .or_default()
            .extend(page.items.iter().map(|i| i.video_no));
    }
    let empty = HashSet::new();
    let mut stale = HashSet::new();
    for page in pages.iter().filter(|p| p.page == 0) {
        let fresh = incoming.get(page.channel_id.as_str()).unwrap_or(&empty);
        let rows: Vec<&&CachedVod> = groups
            .get(&page.channel_id)
            .map(|v| v.iter().filter(|r| !fresh.contains(&r.video_no)).collect())
            .unwrap_or_default();
        // 피드 순서의 마지막 = 가장 오래된 항목. Swift `max(by: isNewer)` 와 같게 동률이면 뒤의 것.
        let mut oldest: Option<&CachedVodData> = None;
        for item in &page.items {
            oldest = match oldest {
                None => Some(item),
                Some(cur) => {
                    // max(by: areInIncreasingOrder) — cur < item 이면 item 이 새 최대.
                    if subscription_is_newer(
                        cur.publish_date,
                        cur.video_no,
                        item.publish_date,
                        item.video_no,
                    ) {
                        Some(item)
                    } else {
                        Some(cur)
                    }
                }
            };
        }
        match (page.exhausted, oldest) {
            (false, Some(oldest)) => {
                stale.extend(
                    rows.iter()
                        .filter(|r| {
                            subscription_is_newer(
                                r.publish_date,
                                r.video_no,
                                oldest.publish_date,
                                oldest.video_no,
                            )
                        })
                        .map(|r| r.video_no),
                );
            }
            _ => stale.extend(rows.iter().map(|r| r.video_no)),
        }
    }
    stale
}

/// 행에서 `tag` 토큰을 떼고, 남은 토큰이 없으면 지운다(순서 유지). 바뀐 videoNo.
fn remove_token(conn: &Connection, tag: &str, rows: &mut [CachedVod]) -> Result<HashSet<i64>> {
    let mut removed = HashSet::new();
    for row in rows.iter_mut() {
        let tokens: Vec<&str> = row.sources.split(',').collect();
        let kept: Vec<&str> = tokens.iter().copied().filter(|t| *t != tag).collect();
        if kept.len() == tokens.len() {
            continue;
        }
        if kept.is_empty() {
            db::delete(conn, row)?;
        } else {
            row.sources = kept.join(",");
            db::update(conn, row)?;
        }
        removed.insert(row.video_no);
    }
    Ok(removed)
}

impl DataStore {
    /// batch upsert. 반환값은 실제로 변경/추가된 row 수.
    pub fn upsert_cached_vods(&self, items: &[CachedVodData]) -> Result<usize> {
        self.write(|tx| {
            let n = upsert_rows(tx, items)?.0.len();
            Ok((n, n > 0))
        })
    }

    /// 이번 묶음에 없는 행에서 `source` 태그만 뗀다. 빈 묶음은 지우지 않는다.
    pub fn retain_cached_vod_source(
        &self,
        source: &str,
        video_nos: &HashSet<i64>,
    ) -> Result<usize> {
        if source.is_empty() || video_nos.is_empty() {
            return Ok(0);
        }
        self.write(|tx| {
            let rows = db::fetch::<CachedVod>(
                tx,
                "WHERE instr(sources, ?) > 0",
                &[Value::Text(source.into())],
            )?;
            let mut changed = 0usize;
            for mut row in rows {
                if video_nos.contains(&row.video_no) {
                    continue;
                }
                let mut sources = split_sources(&row.sources);
                if !sources.remove(source) {
                    continue;
                }
                if sources.is_empty() {
                    db::delete(tx, &row)?;
                } else {
                    row.sources = join_sources(&sources);
                    db::update(tx, &row)?;
                }
                changed += 1;
            }
            Ok((changed, changed > 0))
        })
    }

    /// source 태그(CSV 정확 일치)가 붙은 VOD — 발행일 최신(없으면 뒤) → 가져온 시각 → videoNo 내림차순.
    pub fn fetch_cached_vods(&self, source: &str, limit: usize) -> Result<Vec<CachedVodData>> {
        if limit == 0 || source.is_empty() {
            return Ok(Vec::new());
        }
        let window = (limit * 4).max(200);
        self.read(|conn| {
            let base = "WHERE instr(sources, ?) > 0 \
                        ORDER BY publishDate IS NULL, publishDate DESC, fetchedAt DESC, videoNo DESC";
            let mut rows = db::fetch::<CachedVod>(
                conn,
                &format!("{base} LIMIT ?"),
                &[Value::Text(source.into()), Value::Integer(window as i64)],
            )?;
            if rows.len() == window
                && rows.iter().filter(|r| has_tag(&r.sources, source)).take(limit).count() < limit
            {
                rows = db::fetch::<CachedVod>(conn, base, &[Value::Text(source.into())])?;
            }
            Ok(rows
                .iter()
                .filter(|r| has_tag(&r.sources, source))
                .take(limit)
                .map(CachedVodData::from)
                .collect())
        })
    }

    /// 계정 토큰 `tag` 가 붙은 행을 채널별로 — 채널 안 피드 순서로 `per_channel_limit` 개까지.
    /// 옛 토큰이나 빈 값은 빈 결과다.
    pub fn fetch_subscription_vods(
        &self,
        tag: &str,
        per_channel_limit: usize,
    ) -> Result<HashMap<String, Vec<CachedVodData>>> {
        if per_channel_limit == 0 || !is_scoped_subscription_tag(tag) {
            return Ok(HashMap::new());
        }
        self.read(|conn| {
            let rows = subscription_rows(conn, tag)?;
            Ok(newest_first_by_channel(&rows)
                .into_iter()
                .map(|(k, v)| {
                    (
                        k,
                        v.into_iter()
                            .take(per_channel_limit)
                            .map(CachedVodData::from)
                            .collect(),
                    )
                })
                .collect())
        })
    }

    /// 채널별 페이지 결과를 계정 사본에 반영한다(한 트랜잭션). 규칙 (a)~(e) 는 원본 주석 그대로:
    ///
    /// (a) 모든 항목을 `tag` 와 함께 upsert(channelId 가 없으면 페이지의 채널).
    /// (b) page 0 이 온 채널: 새 page 0 에 없는데 그 page 0 의 가장 오래된 항목보다 최신인 행에서 태그를 뗀다.
    ///     page 0 이 비었거나 마지막 페이지면 이번에 온 항목 밖의 행을 모두 뗀다.
    /// (c) page ≥ 1 은 추가만 한다.
    /// (d) 채널마다 `per_channel_limit` 을 넘는 가장 오래된 행에서 태그를 뗀다.
    /// (e) `followed` 가 있으면 목록 밖 채널의 행에서 태그를 뗀다.
    ///
    /// 태그가 모두 빠진 행은 지운다. 바뀐 행 수를 돌려준다.
    pub fn apply_subscription_vod_pages(
        &self,
        tag: &str,
        pages: &[SubscriptionVodPage],
        followed: Option<&HashSet<String>>,
        per_channel_limit: usize,
    ) -> Result<usize> {
        if !is_scoped_subscription_tag(tag) || (pages.is_empty() && followed.is_none()) {
            return Ok(0);
        }
        let pages: Vec<SubscriptionVodPage> = match followed {
            Some(ids) => pages
                .iter()
                .filter(|p| ids.contains(&p.channel_id))
                .cloned()
                .collect(),
            None => pages.to_vec(),
        };
        self.write(|tx| {
            let mut tagged: HashMap<i64, CachedVod> = HashMap::new();
            for row in subscription_rows(tx, tag)? {
                tagged.entry(row.video_no).or_insert(row);
            }
            let incoming: Vec<CachedVodData> = pages
                .iter()
                .flat_map(|page| {
                    page.items.iter().map(move |item| {
                        let mut item = item.clone();
                        item.sources.insert(tag.to_string());
                        if item.channel_id.is_none() {
                            item.channel_id = Some(page.channel_id.clone());
                        }
                        item
                    })
                })
                .collect();
            let (upsert_changed, upsert_rows_map) = upsert_rows(tx, &incoming)?;
            tagged.extend(upsert_rows_map);

            let mut untag = {
                let groups = newest_first_by_channel(tagged.values());
                stale_page_zero_rows(&pages, &groups)
            };
            let kept: Vec<&CachedVod> = tagged
                .values()
                .filter(|r| !untag.contains(&r.video_no))
                .collect();
            let mut overflow = Vec::new();
            for rows in newest_first_by_channel(kept).values() {
                overflow.extend(
                    rows.iter()
                        .skip(per_channel_limit.max(1))
                        .map(|r| r.video_no),
                );
            }
            untag.extend(overflow);
            if let Some(followed) = followed {
                untag.extend(
                    tagged
                        .values()
                        .filter(|r| !followed.contains(r.channel_id.as_deref().unwrap_or("")))
                        .map(|r| r.video_no),
                );
            }
            let mut targets: Vec<CachedVod> = untag
                .iter()
                .filter_map(|no| tagged.get(no).cloned())
                .collect();
            let removed = remove_token(tx, tag, &mut targets)?;
            let changed = upsert_changed.union(&removed).count();
            Ok((changed, changed > 0))
        })
    }

    /// 옛 구독 토큰과 `keep` 이 아닌 계정 토큰을 지운다. 지운 행과 토큰을 뗀 행의 수.
    ///
    /// `keep` 이 없거나 계정 토큰이 아니면 옛 토큰만 뗀다. `channel-*` 같은 다른 토큰은 건드리지 않는다.
    pub fn purge_subscription_vod_tags(&self, keep: Option<&str>) -> Result<usize> {
        let legacy = CachedVod::LEGACY_SUBSCRIPTION_TAG;
        let keep_token = keep.filter(|k| is_scoped_subscription_tag(k));
        self.write(|tx| {
            let bare_rows =
                db::count::<CachedVod>(tx, "WHERE sources = ?", &[Value::Text(legacy.into())])?
                    as usize;
            if bare_rows > 0 {
                tx.execute("DELETE FROM CachedVOD WHERE sources = ?", [legacy])?;
            }
            let candidates = db::fetch::<CachedVod>(
                tx,
                "WHERE instr(sources, ?) > 0 AND sources != ?",
                &[Value::Text(legacy.into()), Value::Text(legacy.into())],
            )?;
            let mut changed = bare_rows;
            for mut row in candidates {
                let tokens: Vec<&str> = row.sources.split(',').collect();
                let kept: Vec<&str> = tokens
                    .iter()
                    .copied()
                    .filter(|token| {
                        if !CachedVod::is_subscription_token(token) {
                            return true;
                        }
                        match keep_token {
                            None => *token != legacy,
                            Some(k) => *token == k,
                        }
                    })
                    .collect();
                if kept.len() == tokens.len() {
                    continue;
                }
                if kept.is_empty() {
                    db::delete(tx, &row)?;
                } else {
                    row.sources = kept.join(",");
                    db::update(tx, &row)?;
                }
                changed += 1;
            }
            Ok((changed, changed > 0))
        })
    }

    /// 테스트·진단용: VOD 원시 행.
    #[doc(hidden)]
    pub fn cached_vod_row(&self, video_no: i64) -> Result<Option<CachedVod>> {
        self.read(|conn| {
            Ok(db::fetch_one::<CachedVod>(
                conn,
                "WHERE videoNo = ?",
                &[Value::Integer(video_no)],
            )?)
        })
    }

    /// 테스트·시드용: VOD 행을 그대로 넣는다.
    #[doc(hidden)]
    pub fn insert_cached_vod_rows(&self, rows: &[CachedVod]) -> Result<()> {
        self.write(|tx| {
            for row in rows {
                let mut row = row.clone();
                db::insert(tx, &mut row)?;
            }
            Ok(((), !rows.is_empty()))
        })
    }
}
