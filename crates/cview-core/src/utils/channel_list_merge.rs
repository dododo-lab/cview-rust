//! 채널 상세 VOD·클립 목록의 첫 페이지 갱신과 '더 불러오기' 붙이기(U11).
//!
//! 원본: `Sources/CViewCore/Utilities/ChannelListMerge.swift`
//!
//! 규칙(순수 함수 — 모델은 결과만 대입한다):
//! - 기존 목록이 한 페이지 이하이거나 새 첫 페이지가 비었거나 한 페이지보다 짧으면 교체한다.
//!   짧은 첫 페이지는 "목록 전체가 이것뿐" 이라는 뜻이라 기존 꼬리를 남기면 지워진 항목이 살아남는다.
//! - 새 첫 페이지가 기존 목록과 이어지면(겹침) 새 페이지 + 기존에서 이음매 뒤의 나머지(새 id 제외)다.
//!   이음매는 새 페이지를 뒤에서부터 훑어 처음 만나는 기존 항목이다.
//! - 겹침이 없으면 교체한다 — 사이의 빈틈을 알 길이 없다.
//!
//! 원본의 `Identifiable` 오버로드는 id 클로저(`|x| x.id.clone()`)로 대신한다.

use std::collections::{HashMap, HashSet};
use std::hash::Hash;

/// 첫 페이지 병합 결과. `replaced` 가 true 면 호출부가 페이지 상태를 첫 페이지로 되돌린다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirstPageMerge<T> {
    pub merged: Vec<T>,
    pub replaced: bool,
}

/// 붙이기 결과. `appended` 는 실제로 붙은 개수.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppendResult<T> {
    pub merged: Vec<T>,
    pub appended: usize,
}

/// 첫 페이지 갱신 결과를 기존 누적 목록과 합친다.
pub fn merge_first_page<T: Clone, Id: Hash + Eq>(
    existing: &[T],
    fresh: &[T],
    page_size: usize,
    id: impl Fn(&T) -> Id,
) -> FirstPageMerge<T> {
    let replace = || FirstPageMerge { merged: fresh.to_vec(), replaced: true };
    if existing.len() <= page_size || fresh.is_empty() || fresh.len() < page_size {
        return replace();
    }
    let mut existing_index: HashMap<Id, usize> = HashMap::with_capacity(existing.len());
    for (index, element) in existing.iter().enumerate() {
        existing_index.entry(id(element)).or_insert(index);
    }
    let Some(seam) = fresh.iter().rev().find_map(|e| existing_index.get(&id(e)).copied()) else {
        return replace();
    };
    let fresh_ids: HashSet<Id> = fresh.iter().map(&id).collect();
    let mut merged = Vec::with_capacity(fresh.len() + existing.len() - seam - 1);
    merged.extend_from_slice(fresh);
    merged.extend(existing[seam + 1..].iter().filter(|e| !fresh_ids.contains(&id(e))).cloned());
    FirstPageMerge { merged, replaced: false }
}

/// 다음 페이지를 붙이되 이미 있는 id 는 건너뛴다. 같은 페이지 안의 중복도 하나만 남긴다.
pub fn append_skipping_duplicates<T: Clone, Id: Hash + Eq>(
    existing: &[T],
    page: &[T],
    id: impl Fn(&T) -> Id,
) -> AppendResult<T> {
    let mut seen: HashSet<Id> = existing.iter().map(&id).collect();
    let mut merged = Vec::with_capacity(existing.len() + page.len());
    merged.extend_from_slice(existing);
    merged.extend(page.iter().filter(|e| seen.insert(id(e))).cloned());
    let appended = merged.len() - existing.len();
    AppendResult { merged, appended }
}

#[cfg(test)]
mod tests {
    //! 원본: `Tests/CViewCoreTests/ChannelListMergeTests.swift`
    use super::*;

    #[derive(Debug, Clone, PartialEq)]
    struct Row {
        id: String,
        title: String,
    }

    fn row(id: &str) -> Row {
        Row { id: id.into(), title: String::new() }
    }

    const PAGE: usize = 12;

    fn rows(prefix: &str, range: std::ops::Range<usize>) -> Vec<Row> {
        range.map(|i| row(&format!("{prefix}{i}"))).collect()
    }

    fn ids(rows: &[Row]) -> Vec<String> {
        rows.iter().map(|r| r.id.clone()).collect()
    }

    fn key(r: &Row) -> String {
        r.id.clone()
    }

    fn cat(a: Vec<Row>, b: Vec<Row>) -> Vec<Row> {
        a.into_iter().chain(b).collect()
    }

    #[test]
    fn front_insert_keeps_accumulated() {
        let existing = rows("e", 0..48);
        let fresh = cat(vec![row("n0"), row("n1")], rows("e", 0..10));
        let r = merge_first_page(&existing, &fresh, PAGE, key);
        assert!(!r.replaced);
        assert_eq!(r.merged.len(), 50);
        let mut expected = vec!["n0".to_string(), "n1".to_string()];
        expected.extend(ids(&existing));
        assert_eq!(ids(&r.merged), expected);
    }

    #[test]
    fn deletion_inside_first_page() {
        let existing = rows("e", 0..36);
        let fresh = cat(rows("e", 0..3), rows("e", 4..13));
        let r = merge_first_page(&existing, &fresh, PAGE, key);
        assert!(!r.replaced);
        assert!(!ids(&r.merged).contains(&"e3".to_string()));
        let expected: Vec<String> = ids(&existing).into_iter().filter(|i| i != "e3").collect();
        assert_eq!(ids(&r.merged), expected);
    }

    #[test]
    fn fresh_values_win() {
        let existing = rows("e", 0..24);
        let mut fresh = rows("e", 0..12);
        fresh[2].title = "고친 제목".into();
        let r = merge_first_page(&existing, &fresh, PAGE, key);
        assert!(!r.replaced);
        assert_eq!(r.merged.len(), 24);
        assert_eq!(r.merged[2].title, "고친 제목");
    }

    #[test]
    fn jump_to_front_is_deduplicated() {
        let existing = rows("e", 0..60);
        let fresh = cat(vec![row("e40")], rows("e", 0..11));
        let r = merge_first_page(&existing, &fresh, PAGE, key);
        assert!(!r.replaced);
        assert_eq!(r.merged.len(), 60);
        assert_eq!(ids(&r.merged).iter().filter(|i| *i == "e40").count(), 1);
        assert!(ids(&r.merged).contains(&"e30".to_string()), "이음매 뒤의 누적분을 버리지 않는다");
    }

    #[test]
    fn no_overlap_replaces() {
        let existing = rows("e", 0..36);
        let fresh = rows("n", 0..12);
        let r = merge_first_page(&existing, &fresh, PAGE, key);
        assert!(r.replaced);
        assert_eq!(r.merged, fresh);
    }

    #[test]
    fn short_lists_replace() {
        let one_page = rows("e", 0..12);
        let fresh = cat(vec![row("n0")], rows("e", 0..11));
        let short = merge_first_page(&one_page, &fresh, PAGE, key);
        assert!(short.replaced && short.merged == fresh);

        let existing = rows("e", 0..36);
        let empty = merge_first_page(&existing, &[], PAGE, key);
        assert!(empty.replaced && empty.merged.is_empty());

        let truncated = rows("e", 0..5);
        let whole = merge_first_page(&existing, &truncated, PAGE, key);
        assert!(whole.replaced && whole.merged == truncated);
    }

    #[test]
    fn append_skips_duplicates() {
        let existing = cat(vec![row("n0"), row("n1")], rows("e", 0..10));
        let page = rows("e", 8..20);
        let r = append_skipping_duplicates(&existing, &page, key);
        assert_eq!(r.appended, 10);
        let mut expected = ids(&existing);
        expected.extend(ids(&rows("e", 10..20)));
        assert_eq!(ids(&r.merged), expected);

        let repeated = append_skipping_duplicates(&[], &[row("x"), row("x")], key);
        assert_eq!(ids(&repeated.merged), vec!["x"]);
        assert_eq!(repeated.appended, 1);

        let nothing = append_skipping_duplicates(&existing, &existing, key);
        assert!(nothing.appended == 0 && nothing.merged == existing);
    }

    /// 원본은 `VODInfo` 모델과 `\.videoNo` 키패스로 검증한다. 모델 대신 같은 모양의 행을 쓴다.
    #[test]
    fn works_with_numeric_ids() {
        #[derive(Debug, Clone)]
        struct Vod {
            video_no: i64,
        }
        let vod = |n: i64| Vod { video_no: n };
        let existing: Vec<Vod> = (0..24).map(vod).collect();
        let fresh: Vec<Vod> = std::iter::once(vod(100)).chain((0..11).map(vod)).collect();
        let r = merge_first_page(&existing, &fresh, 12, |v| v.video_no);
        assert!(!r.replaced);
        let expected: Vec<i64> = std::iter::once(100).chain(0..24).collect();
        assert_eq!(r.merged.iter().map(|v| v.video_no).collect::<Vec<_>>(), expected);
        let page: Vec<Vod> = (20..30).map(vod).collect();
        let appended = append_skipping_duplicates(&r.merged, &page, |v| v.video_no);
        assert_eq!(appended.appended, 6);
    }
}
