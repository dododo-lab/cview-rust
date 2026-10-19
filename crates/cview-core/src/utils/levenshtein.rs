//! 검색 정밀화: typo tolerance 용 편집거리 유틸리티.
//!
//! 원본: `Sources/CViewCore/Utilities/Levenshtein.swift`
//!
//! Swift 는 `Character`(확장 grapheme cluster) 단위로 비교하지만 여기서는 `char`
//! (유니코드 스칼라) 단위로 비교한다. 한글 완성형·ASCII 에서는 결과가 같다.

/// Levenshtein 편집거리. O(n·m) DP — 큰 입력에는 호출부에서 budget cap 권장.
pub fn distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (n, m) = (a.len(), b.len());
    if n == 0 {
        return m;
    }
    if m == 0 {
        return n;
    }
    let mut prev: Vec<usize> = (0..=m).collect();
    let mut curr = vec![0usize; m + 1];
    for i in 1..=n {
        curr[0] = i;
        for j in 1..=m {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[m]
}

#[cfg(test)]
mod tests {
    //! 원본: `Tests/CViewCoreTests/LevenshteinTests.swift`
    use super::distance;

    #[test]
    fn identical() {
        assert_eq!(distance("abc", "abc"), 0);
    }
    #[test]
    fn substitution() {
        assert_eq!(distance("abc", "abd"), 1);
    }
    #[test]
    fn insertion() {
        assert_eq!(distance("abc", "abcd"), 1);
    }
    #[test]
    fn korean_one_typo() {
        assert_eq!(distance("우왁굳", "우왁군"), 1);
    }
    #[test]
    fn empty_sides() {
        assert_eq!(distance("", "abc"), 3);
        assert_eq!(distance("ab", ""), 2);
    }
}
