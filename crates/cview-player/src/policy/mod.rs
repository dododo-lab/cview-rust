//! 엔진 비의존 재생 정책·상수 (각 하위 모듈의 머리 주석이 원본 Swift 파일을 밝힌다).

pub mod constants;
pub mod latency_pairing;
pub mod latency_snap;
pub mod manifest_lease;
pub mod manifest_refresh_policy;
pub mod media_response_policy;
pub mod occlusion_cap;
pub mod playback_key_access;
pub mod quality_policy;
pub mod recovery_signal;
pub mod session_tier;

pub use latency_pairing::LatencyPresetPairing;
pub use manifest_lease::{ManifestLease, ManifestLeaseSource};
pub use playback_key_access::{PlaybackCookie, PlaybackKeyAccess};
pub use quality_policy::QualityPolicyMode;
pub use recovery_signal::{RecoveryDecision, RecoverySignal};
pub use session_tier::SessionTier;
