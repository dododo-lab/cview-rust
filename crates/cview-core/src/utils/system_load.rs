//! 시스템 부하 모드 — 동적 throttle 정책의 순수 부분.
//!
//! 원본: `Sources/CViewCore/Utilities/SystemLoadMonitor.swift`
//!
//! `SystemLoadMonitor`(macOS `ProcessInfo.thermalState` · 저전력 모드 감시)는 플랫폼 종속이라
//! 옮기지 않았다. 여기에는 모드 → 조정값 계산만 둔다. 모드 판정 규칙은
//! [`SystemLoadMode::from_thermal_state`] 로 남겨 플랫폼 구현이 재사용할 수 있게 했다.

/// 시스템 부하 모드 — thermalState + lowPowerMode 를 하나로 추상화.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SystemLoadMode {
    /// nominal/fair, 정상 동작
    Normal = 0,
    /// serious, 부하 감소 권고 (fair/nominal + 저전력 모드도 여기)
    Warm = 1,
    /// critical, 강한 throttle 필요
    Hot = 2,
}

/// macOS `ProcessInfo.ThermalState` 대응.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThermalState {
    Nominal,
    Fair,
    Serious,
    Critical,
}

impl SystemLoadMode {
    /// thermalState 우선, lowPowerMode 보강(`SystemLoadMonitor.currentMode` 규칙).
    pub fn from_thermal_state(state: ThermalState, low_power: bool) -> Self {
        match state {
            ThermalState::Critical | ThermalState::Serious => Self::Hot,
            ThermalState::Fair | ThermalState::Nominal => {
                if low_power {
                    Self::Warm
                } else {
                    Self::Normal
                }
            }
        }
    }

    /// 디코더 스레드 수 가이드 — base 를 부하에 따라 줄인다. 예: base=6 → normal=6, warm=4, hot=2
    pub fn adjusted_decoder_threads(self, base: i64) -> i64 {
        let base = base.max(1);
        match self {
            Self::Normal => base,
            Self::Warm => base.min((base / 3 * 2 + base % 3 * 2 / 3).max(2)),
            Self::Hot => (base / 3).max(1),
        }
    }

    /// 채팅 배치 flush 간격 — 부하가 높을수록 간격 증가. 예: base=33ms → normal=33, warm=66, hot=132
    /// (원본은 오버플로 시 트랩하지만 여기서는 포화한다.)
    pub fn adjusted_chat_flush_interval_ns(self, base: u64) -> u64 {
        match self {
            Self::Normal => base,
            Self::Warm => base.saturating_mul(2),
            Self::Hot => base.saturating_mul(4),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_from_thermal_state() {
        assert_eq!(SystemLoadMode::from_thermal_state(ThermalState::Nominal, false), SystemLoadMode::Normal);
        assert_eq!(SystemLoadMode::from_thermal_state(ThermalState::Fair, true), SystemLoadMode::Warm);
        assert_eq!(SystemLoadMode::from_thermal_state(ThermalState::Serious, false), SystemLoadMode::Hot);
        assert_eq!(SystemLoadMode::from_thermal_state(ThermalState::Critical, true), SystemLoadMode::Hot);
        assert!(SystemLoadMode::Normal < SystemLoadMode::Warm && SystemLoadMode::Warm < SystemLoadMode::Hot);
    }

    #[test]
    fn decoder_thread_examples() {
        assert_eq!(SystemLoadMode::Normal.adjusted_decoder_threads(6), 6);
        assert_eq!(SystemLoadMode::Warm.adjusted_decoder_threads(6), 4);
        assert_eq!(SystemLoadMode::Hot.adjusted_decoder_threads(6), 2);
        assert_eq!(SystemLoadMode::Hot.adjusted_chat_flush_interval_ns(33_000_000), 132_000_000);
    }
}
