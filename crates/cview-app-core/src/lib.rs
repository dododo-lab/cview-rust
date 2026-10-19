//! CViewApp 의 UI 비의존 로직(ViewModel·서비스·내비게이션) 포팅. 화면은 `cview-app`(Tauri)이 담당한다.

pub mod actions;
pub mod app_state;
pub mod caching;
pub mod logic;
pub mod navigation;
pub mod services;
pub mod sync;
pub mod viewmodels;
