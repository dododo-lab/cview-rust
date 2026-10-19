//! `Sources/CViewApp/Navigation/PlaybackSurfaceScope.swift` 포팅
//! (+ `CViewCore/Focus/TextEntryFocus.swift` 의 `FrontmostPlaybackSurface`).
//!
//! 재생 표면이 등장할 때 자기 종류를 "전면" 으로 등록하고 사라질 때 되돌린다. SwiftUI
//! `ViewModifier` 의 `onAppear`/`onDisappear` 는 [`PlaybackSurfaceScope::appear`] /
//! [`PlaybackSurfaceScope::disappear`] 호출로 바뀐다.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FrontmostPlaybackSurface {
    /// 재생 표면이 전면에 없음.
    #[default]
    None,
    Live,
    Vod,
    Clip,
}

impl FrontmostPlaybackSurface {
    /// 메인 메뉴의 재생 명령이 이 표면을 조작해도 되는가 — 라이브만.
    pub fn accepts_global_playback_commands(self) -> bool {
        self == Self::Live
    }

    /// `CViewApp.acceptsGlobalPlaybackCommands` — `.none` 도 허용한다.
    pub fn allows_menu_playback(self) -> bool {
        matches!(self, Self::None | Self::Live)
    }
}

/// 표면 하나의 등록 범위. 등장 직전 값을 기억해 중첩(라이브 → 클립 모달)을 되돌린다.
#[derive(Debug, Clone, Copy)]
pub struct PlaybackSurfaceScope {
    surface: FrontmostPlaybackSurface,
    previous: FrontmostPlaybackSurface,
}

impl PlaybackSurfaceScope {
    /// 표면 등장 — 전면 값을 바꾸고 범위를 돌려준다.
    pub fn appear(
        surface: FrontmostPlaybackSurface,
        frontmost: &mut FrontmostPlaybackSurface,
    ) -> Self {
        let previous = *frontmost;
        *frontmost = surface;
        Self { surface, previous }
    }

    /// 표면 소멸 — 그 사이 다른 표면이 올라탔으면 건드리지 않는다(마지막 등록자가 소유).
    pub fn disappear(self, frontmost: &mut FrontmostPlaybackSurface) {
        if *frontmost == self.surface {
            *frontmost = self.previous;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_scope_restores_previous() {
        let mut front = FrontmostPlaybackSurface::None;
        let live = PlaybackSurfaceScope::appear(FrontmostPlaybackSurface::Live, &mut front);
        let clip = PlaybackSurfaceScope::appear(FrontmostPlaybackSurface::Clip, &mut front);
        assert!(!front.allows_menu_playback());
        clip.disappear(&mut front);
        assert_eq!(front, FrontmostPlaybackSurface::Live);
        live.disappear(&mut front);
        assert_eq!(front, FrontmostPlaybackSurface::None);
    }

    #[test]
    fn stale_disappear_does_not_clobber_newer_surface() {
        let mut front = FrontmostPlaybackSurface::None;
        let vod = PlaybackSurfaceScope::appear(FrontmostPlaybackSurface::Vod, &mut front);
        let _clip = PlaybackSurfaceScope::appear(FrontmostPlaybackSurface::Clip, &mut front);
        vod.disappear(&mut front);
        assert_eq!(front, FrontmostPlaybackSurface::Clip);
    }
}
