//! `Sources/CViewCore/Models/EqualizerPreset.swift` 포팅 — 10밴드 이퀄라이저 프리셋.
//!
//! 밴드 구성은 JS 그래프(`EQ_BAND_FREQUENCIES`)와 반드시 같은 순서·같은 개수여야 한다.

/// 10밴드 EQ 프리셋. 이득 단위는 dB.
#[derive(Debug, Clone, PartialEq)]
pub struct EqualizerPreset {
    pub name: &'static str,
    /// 밴드별 이득(dB). 길이는 항상 [`EqualizerPreset::FREQUENCIES`] 와 같다.
    pub gains: [f32; 10],
    /// 프리앰프(dB).
    pub pre_amp: f32,
}

const fn preset(name: &'static str, gains: [f32; 10]) -> EqualizerPreset {
    EqualizerPreset {
        name,
        gains,
        pre_amp: 0.0,
    }
}

impl EqualizerPreset {
    /// 밴드 중심 주파수(Hz). JS `EQ_BAND_FREQUENCIES` 와 동일해야 한다.
    pub const FREQUENCIES: [f32; 10] = [
        31.0, 62.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0,
    ];

    /// 기본 제공 프리셋. 첫 항목은 항상 평탄(= EQ 를 켜되 소리를 바꾸지 않음)이다.
    pub const ALL: [EqualizerPreset; 10] = [
        preset("평탄", [0.0; 10]),
        preset("록", [5.0, 4.0, 3.0, 1.0, -1.0, -1.0, 1.0, 3.0, 4.0, 5.0]),
        preset("팝", [-1.0, 1.0, 3.0, 4.0, 4.0, 2.0, 0.0, -1.0, -1.0, -1.0]),
        preset("재즈", [4.0, 3.0, 1.0, 2.0, -1.0, -1.0, 0.0, 1.0, 3.0, 4.0]),
        preset(
            "클래식",
            [4.0, 3.0, 2.0, 0.0, 0.0, 0.0, -1.0, -2.0, -2.0, -3.0],
        ),
        preset(
            "댄스",
            [6.0, 5.0, 2.0, 0.0, 0.0, -2.0, -3.0, -3.0, 0.0, 0.0],
        ),
        preset(
            "저음 강화",
            [7.0, 6.0, 5.0, 3.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        ),
        preset(
            "고음 강화",
            [0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 3.0, 5.0, 6.0, 7.0],
        ),
        // 사람 목소리 대역(≈250Hz~4kHz)을 올리고 양 끝을 낮춘다 — 잡담 방송용.
        preset(
            "보컬",
            [-3.0, -2.0, 0.0, 3.0, 4.0, 4.0, 3.0, 1.0, -1.0, -2.0],
        ),
        preset(
            "헤드폰",
            [4.0, 3.0, 1.0, -1.0, -2.0, -1.0, 1.0, 3.0, 5.0, 6.0],
        ),
    ];

    pub fn id(&self) -> &'static str {
        self.name
    }

    pub fn names() -> Vec<&'static str> {
        Self::ALL.iter().map(|p| p.name).collect()
    }

    pub fn preset_named(name: &str) -> Option<&'static EqualizerPreset> {
        Self::ALL.iter().find(|p| p.name == name)
    }

    /// 어떤 프리셋과도 일치하지 않으면 `None` — UI 가 "사용자 지정" 으로 표시한다.
    pub fn matching(gains: &[f32], pre_amp: f32) -> Option<&'static EqualizerPreset> {
        Self::ALL
            .iter()
            .find(|p| p.gains.as_slice() == gains && p.pre_amp == pre_amp)
    }
}

#[cfg(test)]
mod tests {
    //! `EqualizerPresetTests.swift` 의 불변식.
    use super::*;

    #[test]
    fn invariants() {
        assert_eq!(EqualizerPreset::FREQUENCIES.len(), 10);
        assert_eq!(EqualizerPreset::ALL[0].name, "평탄");
        assert!(EqualizerPreset::ALL[0].gains.iter().all(|g| *g == 0.0));
        let names = EqualizerPreset::names();
        let unique: std::collections::HashSet<_> = names.iter().collect();
        assert_eq!(unique.len(), names.len());
        assert_eq!(EqualizerPreset::preset_named("록").unwrap().gains[0], 5.0);
        assert!(EqualizerPreset::preset_named("없음").is_none());
        let rock = EqualizerPreset::preset_named("록").unwrap();
        assert_eq!(
            EqualizerPreset::matching(&rock.gains, 0.0).unwrap().name,
            "록"
        );
        assert!(EqualizerPreset::matching(&rock.gains, 1.0).is_none());
    }
}
