//! 멀티라이브 레이아웃 모델과 전환 판정.
//!
//! 원본: `Sources/CViewApp/ViewModels/MultiLiveLayoutModePolicy.swift`
//! (`MultiLiveLayoutModeTransition`·`MultiLiveTileDrop`) 와
//! `Sources/CViewApp/ViewModels/MultiLiveSession.swift` 하단의 `MultiLiveGridLayoutMode`·
//! `MultiLiveFocusDirection`·`MultiLiveLayoutRatios`·`MultiLiveLayoutPrefs`.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 그리드 레이아웃 모드.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MultiLiveGridLayoutMode {
    #[default]
    Preset,
    Custom,
    /// 포커스 레이아웃 (1+N) — 방향/비율/메인 채널은 manager 상태로 분리.
    Focus,
}

impl MultiLiveGridLayoutMode {
    pub fn raw_value(self) -> &'static str {
        match self {
            Self::Preset => "preset",
            Self::Custom => "custom",
            Self::Focus => "focus",
        }
    }
}

/// 포커스 메인 패인 방향.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MultiLiveFocusDirection {
    #[default]
    Left,
    Right,
    Top,
    Bottom,
}

impl MultiLiveFocusDirection {
    pub const ALL: [Self; 4] = [Self::Left, Self::Right, Self::Top, Self::Bottom];

    pub fn raw_value(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
            Self::Top => "top",
            Self::Bottom => "bottom",
        }
    }

    pub fn from_raw(raw: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|d| d.raw_value() == raw)
    }

    pub fn is_horizontal(self) -> bool {
        matches!(self, Self::Left | Self::Right)
    }

    pub fn main_at_start(self) -> bool {
        matches!(self, Self::Left | Self::Top)
    }
}

/// 분할 비율.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MultiLiveLayoutRatios {
    /// 2채널 좌우 분할 / 커스텀 상단행 열 비율.
    pub horizontal_ratio: f64,
    /// 3-4채널 상/하 행 비율.
    pub vertical_ratio: f64,
    /// 커스텀 하단행 열 비율.
    pub bottom_row_ratio: f64,
    /// 포커스 메인 패인 비율.
    pub focus_ratio: f64,
}

impl Default for MultiLiveLayoutRatios {
    fn default() -> Self {
        Self {
            horizontal_ratio: 0.5,
            vertical_ratio: 0.5,
            bottom_row_ratio: 0.5,
            focus_ratio: 0.7,
        }
    }
}

impl MultiLiveLayoutRatios {
    pub const MIN_RATIO: f64 = 0.2;
    pub const MAX_RATIO: f64 = 0.8;
    pub const FOCUS_MIN: f64 = 0.45;
    pub const FOCUS_MAX: f64 = 0.85;

    /// `MultiLiveLayoutRatios(focusRatio:)` — 나머지는 기본값.
    pub fn with_focus_ratio(focus_ratio: f64) -> Self {
        Self {
            focus_ratio,
            ..Self::default()
        }
    }

    pub fn clamped(value: f64) -> f64 {
        value.clamp(Self::MIN_RATIO, Self::MAX_RATIO)
    }

    pub fn clamped_focus(value: f64) -> f64 {
        value.clamp(Self::FOCUS_MIN, Self::FOCUS_MAX)
    }
}

/// 마지막 사용 레이아웃 (UserDefaults `multiLiveLayoutPrefs` JSON).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MultiLiveLayoutPrefs {
    /// "preset" | "custom" | "focus".
    pub grid_layout_mode: String,
    pub horizontal_ratio: f64,
    pub vertical_ratio: f64,
    pub bottom_row_ratio: f64,
    pub focus_ratio: f64,
    /// "left" | "right" | "top" | "bottom".
    pub focus_direction: String,
}

impl MultiLiveLayoutPrefs {
    pub const USER_DEFAULTS_KEY: &'static str = "multiLiveLayoutPrefs";

    pub fn new(
        mode: MultiLiveGridLayoutMode,
        ratios: MultiLiveLayoutRatios,
        direction: MultiLiveFocusDirection,
    ) -> Self {
        Self {
            grid_layout_mode: mode.raw_value().into(),
            horizontal_ratio: ratios.horizontal_ratio,
            vertical_ratio: ratios.vertical_ratio,
            bottom_row_ratio: ratios.bottom_row_ratio,
            focus_ratio: ratios.focus_ratio,
            focus_direction: direction.raw_value().into(),
        }
    }

    pub fn parsed_grid_layout_mode(&self) -> MultiLiveGridLayoutMode {
        match self.grid_layout_mode.as_str() {
            "custom" => MultiLiveGridLayoutMode::Custom,
            "focus" => MultiLiveGridLayoutMode::Focus,
            _ => MultiLiveGridLayoutMode::Preset,
        }
    }

    pub fn parsed_focus_direction(&self) -> MultiLiveFocusDirection {
        MultiLiveFocusDirection::from_raw(&self.focus_direction).unwrap_or_default()
    }

    /// 같은 값이면 쓰지 않는다 (`save()` 의 `Self.load() == self` 가드).
    pub fn save(&self, store: &dyn LayoutPrefsStore) {
        if Self::load(store).as_ref() == Some(self) {
            return;
        }
        match serde_json::to_vec(self) {
            Ok(data) => store.set(Self::USER_DEFAULTS_KEY, data),
            Err(e) => tracing::warn!("레이아웃 선호 저장 실패: {e}"),
        }
    }

    pub fn load(store: &dyn LayoutPrefsStore) -> Option<Self> {
        let data = store.get(Self::USER_DEFAULTS_KEY)?;
        serde_json::from_slice(&data).ok()
    }
}

/// UserDefaults 의 바이트 저장소 부분.
pub trait LayoutPrefsStore: Send + Sync {
    fn get(&self, key: &str) -> Option<Vec<u8>>;
    fn set(&self, key: &str, data: Vec<u8>);
}

/// 메모리 저장소 (테스트·기본값).
#[derive(Debug, Default)]
pub struct MemoryLayoutPrefsStore {
    data: Mutex<std::collections::HashMap<String, Vec<u8>>>,
    writes: Mutex<usize>,
}

impl MemoryLayoutPrefsStore {
    pub fn write_count(&self) -> usize {
        *self.writes.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl LayoutPrefsStore for MemoryLayoutPrefsStore {
    fn get(&self, key: &str) -> Option<Vec<u8>> {
        self.data
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(key)
            .cloned()
    }

    fn set(&self, key: &str, data: Vec<u8>) {
        *self.writes.lock().unwrap_or_else(|e| e.into_inner()) += 1;
        self.data
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(key.into(), data);
    }
}

/// `MultiLiveLayoutModeTransition`.
pub mod layout_mode_transition {
    use super::MultiLiveGridLayoutMode;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Outcome {
        Noop,
        Enter(MultiLiveGridLayoutMode),
    }

    pub fn resolve(
        current: MultiLiveGridLayoutMode,
        requested: MultiLiveGridLayoutMode,
        needs_repair: bool,
    ) -> Outcome {
        if current == requested && !needs_repair {
            return Outcome::Noop;
        }
        Outcome::Enter(requested)
    }
}

/// `MultiLiveTileDrop` — 큰 화면이 걸리면 맞바꾸고, 작은 화면끼리는 순서만 옮긴다.
pub mod tile_drop {
    use super::Uuid;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Outcome {
        Swap(usize, usize, Uuid),
        Move { from: usize, to_offset: usize },
    }

    pub fn resolve(
        order: &[Uuid],
        dragged: Uuid,
        target: Uuid,
        main_id: Option<Uuid>,
    ) -> Option<Outcome> {
        if dragged == target {
            return None;
        }
        let from = order.iter().position(|id| *id == dragged)?;
        let to = order.iter().position(|id| *id == target)?;
        if let Some(main_id) = main_id {
            if dragged == main_id || target == main_id {
                let new_main = if dragged == main_id { target } else { dragged };
                return Some(Outcome::Swap(from, to, new_main));
            }
        }
        Some(Outcome::Move {
            from,
            to_offset: if to > from { to + 1 } else { to },
        })
    }

    /// Swift `Array.move(fromOffsets: IndexSet(integer: from), toOffset:)`.
    pub fn apply_move<T>(items: &mut Vec<T>, from: usize, to_offset: usize) {
        if from >= items.len() {
            return;
        }
        let item = items.remove(from);
        let dest = if to_offset > from {
            to_offset - 1
        } else {
            to_offset
        };
        items.insert(dest.min(items.len()), item);
    }
}
