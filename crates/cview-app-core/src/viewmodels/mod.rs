//! `Sources/CViewApp/ViewModels` 포팅.
//!
//! SwiftUI `@Observable` 클래스는 UI 비의존 "상태 + 동작" 으로 옮겼다. 각 뷰모델은
//! [`support::Observed`] 로 상태를 감싸 잠금 해제마다 `tokio::sync::watch` 리비전을 올리고,
//! `snapshot()` 이 `serde::Serialize` 값을 돌려준다 — Tauri 셸은 리비전을 구독해 스냅샷을
//! 프런트엔드로 보내고, 사용자 동작은 메서드 호출로 넘긴다.
//!
//! 버린 것: 애니메이션·트랜잭션, 색·SF Symbol, 뷰 기하, signpost 계측.

pub mod all_broadcasts;
pub mod category_browse;
pub mod channel_detail;
pub mod chat;
pub mod following;
pub mod home;
pub mod multi_live;
pub mod search;
pub mod support;
