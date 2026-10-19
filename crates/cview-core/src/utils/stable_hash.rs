//! 결정적 문자열 해시 (FNV-1a 64bit).
//!
//! 원본: `Sources/CViewCore/Utilities/StableHash.swift`
//!
//! 프로세스 간 일관성이 필요한 UI 파생 인덱스(색/아이콘 선택)용. 암호학적 용도 아님.

/// FNV-1a 64bit 해시. 재시작 후에도 같은 문자열 → 같은 해시.
pub fn fnv1a(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// 문자열을 `[0, count)` 인덱스로 매핑.
///
/// # Panics
/// `count == 0` 이면 패닉(원본의 `precondition`).
pub fn index(s: &str, count: usize) -> usize {
    assert!(count > 0, "count must be > 0");
    (fnv1a(s) % count as u64) as usize
}

#[cfg(test)]
mod tests {
    //! 원본: `Tests/CViewCoreTests/StableHashTests.swift`
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn empty_string_offset_basis() {
        assert_eq!(fnv1a(""), 0xcbf29ce484222325);
    }

    #[test]
    fn determinism() {
        for s in ["리그 오브 레전드", "스타크래프트", "발로란트", "", "A", "🎮"] {
            assert_eq!(fnv1a(s), fnv1a(s));
        }
    }

    #[test]
    fn collision_sanity() {
        let unique: HashSet<u64> =
            ["a", "b", "c", "ab", "ba", "리그", "발로", "스타"].iter().map(|s| fnv1a(s)).collect();
        assert_eq!(unique.len(), 8);
    }

    #[test]
    fn known_vectors() {
        assert_eq!(fnv1a("a"), 0xaf63dc4c8601ec8c);
        assert_eq!(fnv1a("foobar"), 0x85944171f73967e8);
    }

    #[test]
    fn index_bounds() {
        for s in ["카테고리1", "category_xyz", "", "a"] {
            for count in [1, 3, 8, 16, 100] {
                assert!(index(s, count) < count);
            }
        }
    }

    #[test]
    fn index_deterministic() {
        let runs: HashSet<usize> = (0..100).map(|_| index("발로란트", 8)).collect();
        assert_eq!(runs.len(), 1);
    }
}
