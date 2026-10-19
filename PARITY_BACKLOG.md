# Swift ↔ Rust(Tauri) 기능 동등성 백로그

기준: `Sources/` Swift macOS 앱과 `rust/` Tauri 앱(`rust/app-ui` 정적 JS, `crates/cview-app/src/commands/*.rs`, `cview-app-core` 뷰모델)을 2026-10 시점으로 비교했다.
`PORTING_PLAN.md` 이후 Rust UI 에 들어간 것(홈 재설계, 사이드바, 라이브 메뉴, 멀티라이브 그리드/포커스, 스플리터, 테마 3종, 후원·구독 카드/랭킹/스트림 알림)은 "있음"으로 본다.

## 진행 표시

✅ 완료 · 🟡 부분 · 🧪 구현됨·기능 테스트 대기(마지막에 한 번에 확인) · ⏸ 보류 — 항목 이름 앞에 붙인다. 커밋은 `git log --grep <ID>` 로 찾는다.

## 지금 Rust 가 가진 것 (요약)

- **Tauri 명령**: `auth_status/login/logout`, `following_lives`, `popular_lives`, `search`(라이브+채널만), `channel_info`, `live_detail`, `player_start/stop/set_volume/message`, `chat_connect/disconnect/send`, `chat_donation_ranking`, `multilive_*`(add/remove/select/layout/focus/stage_size/stop_all/audio_suspended/player_message), `settings_get/update`(도메인 `player`·`general`·`appearance`·`chat`), `appearance_set_theme`.
- **화면**: 홈, 팔로잉(라이브 격자), 인기(격자), 라이브(시청 화면/이어 볼 채널), 멀티라이브(그리드·포커스, 채널 추가 피커, 선택 패인 채팅), 검색(라이브·채널), 설정(계정·테마·스트림 알림·플레이어 토글 3개·프록시·일반 토글 2개).
- **단축키**: `⌘/Ctrl+K`(검색 화면으로), `⌘/Ctrl+\`(사이드바 접기), `Esc`(시청 화면 닫기)뿐.
- **player.html 에 이미 있으나 셸에서 안 부르는 함수**: `setEqualizerEnabled/Band/PreAmp`, `applyVideoAdjust`, `setSharpPixelScaling`, `setPictureEnhancement`(자동 보정·업스케일), `takeScreenshot`, `startRecording/stopRecording`, `getLevels/requestManualLevel/setForcedHeight`, `setRate`, `seekToLiveEdge`, `seekTimeshift`, `setForcedAspectRatio`.
- **UI 없이 이식된 로직**: `ChannelDetailModel`, `ClipBrowserViewModel`/`ClipPlayerViewModel`, `VodBrowseViewModel`/`VodPlayerViewModel`/`VodChannelBrowser`, `CategoryBrowseViewModel`, `SearchViewModel`(최근 검색·결과 캐시), 채팅 VM 부속(`autocomplete`, `moderation`, `mention`, `read_divider`, `stats`, `tts`), `MentionInbox`/`MultiChatSearchStore`, `MultiChatWorkspaceStore`/`MultiChatPreferences`, `merged_chat_merge_state`, `MultiLiveProposer`, `ChannelPresetStore`, `FavoritesStore`/`RecentChannelsStore`, `NotificationService`(Noop)·`BackgroundUpdateService`, `navigation/deep_link`·`router`, `actions/registry`(커맨드 팔레트 액션), `UpdateService`, `SingleInstanceGuard`, `NetworkPathMonitor`, `BootProgress`, `media_download`·`DownloadHistoryStore`, persistence 의 `fetch_memo/save_memo`, `save_bookmark`, `toggle_favorite`, `aggregate_insights`/`watch_time_by_*`, `start/end_watch_record`.

크기: **S** ≈ 반나절~1일(명령 1~2개 + 작은 UI), **M** ≈ 2~4일(새 화면 하나 또는 명령 묶음), **L** ≈ 1주 이상(여러 화면·새 엔진·플랫폼 연동).

---

## 1. 탐색·내비게이션 (NAV)

| ID | 항목 | Swift 원본 | Rust 현황 | 크기 |
|---|---|---|---|---|
| NAV-01 | ✅ 채널 상세 화면(헤더·팔로워·방송 중 배지·정보/VOD/클립 탭) | `ChannelInfoView`, `ChannelInfoHeaderView`, `ChannelInfoTabContent`, `ChannelVODClipTab`, `ChannelMediaCards` | `channel_info` 명령 + `ChannelDetailModel` VM, 화면 없음 | M |
| NAV-02 | ✅ 카테고리 둘러보기(카테고리 카드 → 카테고리별 라이브, 정렬·고정) | `CategoryBrowseView`, `Category/CategoryBrowseCards`, `CategoryBrowseModels` | `CategoryBrowseViewModel`, `category_groups`, `pinned_category_decoder`, 명령·화면 없음 | M |
| NAV-03 | ✅ 인기(전체방송) 화면 고도화: 30개 페이지네이션, 정렬, 카테고리 필터, 미리보기 인스펙터 | `AllBroadcasts/AllBroadcastsView`, `BroadcastPreviewInspector`, `Components/PaginationBar` | `AllBroadcastsViewModel` 사용, 단순 격자만 | M |
| NAV-04 | 🟡 팔로잉 허브: 오프라인 채널 포함 목록, 정렬(시청자/이름/최근), 카테고리 필터, 간결 보기, 채널 독 | `FollowingView*`, `Following/FollowingChannelSidebar`, `FollowingChannelDenseRow`, `FollowingChannelDock`, `FollowingSortOrder` | `FollowingStore`·`following_browser_query`·`following_channel_sidebar` 로직, UI 는 방송 중 격자만 | M |
| NAV-05 | 🧪 팔로잉 인사이트 레일·스마트 큐(이 페이지 담기 → 멀티뷰) | `Following/FollowingLiveInsightRail`, `ResidentOverviewStage`, `FollowingView+MultiLive` | `following_overview_list_query`, 없음 | M |
| NAV-06 | 🟡 통합 검색: VOD·클립 결과, 최근 검색어, 추천어, 결과 정렬·"전체 보기" | `SearchViews`, `Search/SearchContentView+*`, `SearchResultRows` | `search` 명령은 라이브+채널만, `SearchViewModel`(최근 검색) 미사용, `search_videos` API 미사용 | M |
| NAV-07 | 🧪 검색 디스커버리(태그 칩, 인사이트 레일) | `Search/LiveDiscoveryWorkspace`, `DiscoveryInsight*`, `DiscoveryDesign` | 없음 | M |
| NAV-08 | ✅ 라이브러리 허브(클립·다시보기·받은 영상 탭) | `LibraryHubView` | 없음 | S |
| NAV-09 | ✅ 최근 시청·즐겨찾기 화면(즐겨찾기 토글, 최근 기록 삭제) | `RecentFavoritesView` | `FavoritesStore`·`RecentChannelsStore`·persistence `toggle_favorite`; UI 는 localStorage 최근 시청만 | M |
| NAV-10 | 🟡 카드 우클릭 메뉴(재생/멀티라이브 추가/채팅만 열기/채널 상세/즐겨찾기/링크 복사/치지직에서 열기) | `HomeV3/LiveCardActionMenu`, `ClipboardActions` | 카드에 "멀티라이브에 추가" 버튼만 | S |
| NAV-11 | ✅ 뒤로 가기 내비게이션 스택(`⌘[`) | `AppRouter`, `Navigation/*` | `navigation/router.rs`, UI 는 단일 라우트 | S |
| NAV-12 | ✅ 커맨드 팔레트(`⌘K`): 채널·화면·액션 검색 실행 | `CommandPaletteView`, `Actions/CViewActionRegistry` | `actions/registry.rs`·`cview_action.rs`; `⌘K` 는 검색 화면 이동만 | M |
| NAV-13 | 🧪 사이드바 빠른 액션·위젯·재생 인사이트 | `Components/SidebarQuickActions`, `SidebarWidgets`, `SidebarPlaybackInsights` | `sidebar_quick_action.rs`, 없음 | S |
| NAV-14 | 🟡 홈 보강: 인기 클립 레일, 이어보기(VOD 진행률), 빠른 멀티 묶음, 타임라인 | `HomeV3/HomeV3Rail`, `HomeV3QuickMulti`, `HomeV3Timeline`, `HomeV3Discovery` | 홈 VM(`recommendation`, `promotion`) 일부만 화면 반영 | M |
| NAV-15 | 🧪 라이브 허브 모드 전환(탐색/시청/멀티/검색/인기/카테고리, `⌘⌥1~6`) | `MainShell/MenuHubSegment`, `MenuHubTransition`, `navigation/hub_mode` | `hub_mode.rs`, 없음 | S |
| NAV-16 | 🧪 워크스페이스 복원(마지막 화면·채널·멀티 구성) | `Persistence/WorkspaceSnapshot`, `AppState+StoreRecovery` | `workspace_snapshot.rs`, 미연결 | M |

## 2. 라이브 시청 화면·플레이어 (PLY)

| ID | 항목 | Swift 원본 | Rust 현황 | 크기 |
|---|---|---|---|---|
| PLY-01 | ✅ 화질 선택기(자동/1080p/720p…, 현재 화질 표시) | `LiveStreamView`, `PlayerSettingsTab`, `MultiLiveOverlays`("재생 품질") | player.html `getLevels/requestManualLevel/setForcedHeight`, 상태줄 텍스트만 | S |
| PLY-02 | ✅ 재생/일시정지·라이브로 복귀 버튼, 지연 배지 | `LiveStreamView+Logic`, `LiveStreamHeader` | `seekToLiveEdge` 있음, 버튼 없음(지연은 상태 텍스트) | S |
| PLY-03 | 🟡 라이브 되감기(타임시프트) 레일·`←/→` 10초, `⇧←/→` 1분 | `LiveRewindRail`, `TimelineSlider`, `StreamCoordinator+Timeshift` | 서버 타임머신·`seekTimeshift` 있음, 로컬 녹화기 미등록, UI 없음 | L |
| PLY-04 | ✅ 재생 속도 조절 | `PlayerAdvancedSettings+MediaTabs`(재생 탭) | `setRate` 있음, UI 없음 | S |
| PLY-05 | ✅ 플레이어 고급 설정 패널(이퀄라이저/비디오/화면/재생/지연/도구 탭) | `PlayerAdvancedSettingsView`, `PlayerAdvancedSettings+*`, `MLSettingsWindowView`, `ML*Tab` | 없음 | L |
| PLY-06 | 🟡 이퀄라이저(프리셋·프리앰프·10밴드) | `PlayerAdvancedSettings+Equalizer`, `MLEqualizerTab`, `EqualizerPreset` | player.html `setEqualizer*`, `can_use_equalizer` VM, 설정 키 저장만 | M |
| PLY-07 | ✅ 비디오 보정(밝기·대비·채도·색조·감마), 선명한 화면, 자동 보정, 업스케일 | `MLVideoTab`, `PlayerViewModel+AudioVideoAdjust` | player.html `applyVideoAdjust`·`setSharpPixelScaling`·`setPictureEnhancement` | M |
| PLY-08 | ✅ 화면 비율 강제 | `PlayerAdvancedSettings+MediaTabs`(화면) | `setForcedAspectRatio` 있음 | S |
| PLY-09 | ✅ 스크린샷 저장(`⌘S`, PNG/JPEG, 저장 경로) | `PlayerViewModel+Screenshot`, `HLSJSPlayerEngine+Screenshot`, `ScreenshotEncoder` | `takeScreenshot` + `can_take_screenshot`, 파일 저장 명령 없음 | S |
| PLY-10 | 🟡 녹화 시작/중지(`⌥⌘R`) | `PlayerViewModel+Recording`, `HLSJSRecordingWriter`, `StreamRecordingService` | `startRecording/stopRecording`(MediaRecorder), 저장 명령 없음 | M |
| PLY-11 | ✅ 전체 화면 모드 2종(영상만 / 영상+채팅), `⇧⌘F`·`⌃⌘F` | `LiveFullscreenPresentation` | 플레이어 영역 전체화면 버튼만 | S |
| PLY-12 | ✅ 채팅 표시 모드(사이드/오버레이/숨김) + 오버레이 위치·크기·투명도 | `ChatOverlayView`, `ChatDisplayMode`, 설정 `overlay*` | 사이드 고정 | M |
| PLY-13 | ✅ 로딩/오류/종료 오버레이 고도화(재시도, 다시 확인, 브랜드 로딩) | `StreamLoadingOverlay`, `BrandedLoadingView`, `ErrorStateView` | 스피너+문구 오버레이 | S |
| PLY-14 | ✅ 헤더 채널 레일(다른 라이브 채널 전환, 채널 관리 패널) | `LiveChannelHeaderRail`, `LiveChannelHeaderControl`, `LiveChannelManagementPanel` | 아바타·제목·메타만 | M |
| PLY-15 | ✅ 오버레이 볼륨 슬라이더(스크롤 조절, % 표시) | `OverlayVolumeSlider`, `Components/VolumeSymbol` | 하단 range 슬라이더 | S |
| PLY-16 | ✅ 스트림 알림 위치·크기 조정(위치 조정 모드) | `StreamAlertOverlayView`, 설정 `streamAlertAnchorX/Y/Scale` | 알림은 있음, 위치 고정 | S |
| PLY-17 | ✅ 스트림 북마크(`bookmarkStream` 단축키, 북마크 목록·삭제) | `ShortcutAction.bookmarkStream`, `DataStore+Bookmarks` | persistence `save_bookmark/fetch_bookmarks`, 명령 없음 | S |
| PLY-18 | 🧪 별도 플레이어 창(`⌘N`, 창 분리) | `CViewApp` `player-window` | 단일 창 | M |
| PLY-19 | ✅ 백그라운드 재생 끄기/가림 시 일시정지·화질 상한 | 설정 `continuePlaybackInBackground`, `OcclusionResolutionCap` | 미구현 | S |
| PLY-20 | ✅ 시청 기록 기록(시작/종료) | `WatchHeartbeatCenter`, `DataStore+WatchHistorySync` | persistence `start/end_watch_record`, 셸 미호출 | S |

## 3. VOD·클립·다운로드 (VOD)

| ID | 항목 | Swift 원본 | Rust 현황 | 크기 |
|---|---|---|---|---|
| VOD-01 | 🟡 VOD 목록(팔로우 채널 다시보기, 이어보기, 검색, 나중에 보기) | `VODBrowseView`, `VOD/VODMediaCard`, `VODHeroComponents`, `VODPreviewInspector` | `VodBrowseViewModel`, `vod_feed`, `vod_sync_*`, persistence `vod_watch_later`; 명령·화면 없음 | L |
| VOD-02 | ✅ VOD 플레이어(시크바, 속도, 화질, 이어보기 저장) | `VODPlayerView`, `TimelineSlider`, `VODStreamResolver` | `VodPlayerViewModel`·`VodProgressPersistenceWriter`, `VodPlayerEngine` 트레잇만 — WebView `<video>`/hls.js 엔진 필요 | L |
| VOD-03 | ✅ 클립 플레이어(임베드/스트림, 필름스트립 독, 다음 클립) | `ClipPlayerView`, `ClipEmbedWebView`, `ClipFilmstripDock` | `ClipPlayerViewModel`, `clip_detail`/`clip_stream_url` API | M |
| VOD-04 | ✅ 인기 클립 화면(기간 필터, 정렬, 히어로, 미리보기) | `PopularClipsView`, `PopularClipCards`, `Clips/ClipHeroComponents`, `ClipPreviewInspector` | `ClipBrowserViewModel`, `home_popular_clips` | M |
| VOD-05 | ✅ 채널 상세의 VOD·클립 탭 | `ChannelVODClipTab` | `VodChannelBrowser`, `clip_list`/`vod_list` API | M |
| VOD-06 | 🧪 다운로드(VOD/클립 HLS 세그먼트 결합 저장, 진행률, 취소·재시도) | `VideoDownloadService`, `VideoDownloadButton`, `VideoDownloadViews` | `media_download`(작업 모델·폴더 규칙)만, 다운로더 없음 | L |
| VOD-07 | 🧪 다운로드 보관함(진행 중/완료, 끝난 항목 지우기, 폴더 열기) | `DownloadsLibraryView`, `DownloadHistoryStore` | `DownloadHistoryStore`, persistence `download_history` | M |

## 4. 채팅 (CHT)

| ID | 항목 | Swift 원본 | Rust 현황 | 크기 |
|---|---|---|---|---|
| CHT-01 | 🟡 이모티콘 선택기(채널·구독·기본 팩, 검색) | `CViewUI/EmoticonPickerView`, `EmoticonViews` | networking `emoticon_*`, `load_merged_emoticons`; UI 없음 | M |
| CHT-02 | ✅ 채팅 자동완성(`:이모티콘`, `@멘션`) | `ChatAutocompleteView`, `ChatViewModel+Autocomplete` | `viewmodels/chat/autocomplete.rs` | M |
| CHT-03 | ✅ 채팅 컨텍스트 메뉴(메시지/닉네임/ID/금액 복사, 멘션, 핀, 차단, 신고) | `ChatContextMenu` | 없음 | S |
| CHT-04 | ✅ 사용자 프로필 시트(배지, 최근 메시지, 차단·멘션) | `ChatUserProfileSheet` | 없음 | M |
| CHT-05 | ✅ 사용자 차단/해제 + 차단 목록 관리 화면 | `BlockedUsersView`, `ChatViewModel+Moderation` | chat VM `moderation`, `ChatModerationService`(cview-chat), 설정 `blockedUsers` 미노출 | S |
| CHT-06 | ✅ 키워드 필터 관리(금지어 목록, 켜기/끄기) | `KeywordFilterView` | 설정 `blockedWords`/`chatFilterEnabled` 로직, UI 없음 | S |
| CHT-07 | ✅ 채팅 명령어(`/mute`·`/ban`·`/slow`·`/clear`·`/notice`·`/filter`·`/export`) | `CViewChat/ChatModerationService` `ChatCommand` | 파서 이식 여부 확인 필요, 입력창 미연결 | S |
| CHT-08 | ✅ 멘션 강조 + 멘션 트레이(모아 보기, 모두 지우기) | `MentionTrayView`, `MentionInbox`, `MentionNotificationService` | `MentionInbox`, chat VM `mention` | M |
| CHT-09 | ✅ 사용자 하이라이트·역할 강조 토글 | `ChatViewModel+Moderation.toggleHighlight` | VM 로직, UI 없음 | S |
| CHT-10 | ✅ 메시지 핀 고정·스트리머 공지 숨기기 | `StreamerNoticeBannerView`, `pinMessage` | 공지 배너 표시만, `PinnedNoticeCache` | S |
| CHT-11 | ✅ 읽음 구분선·"맨 아래로" 버튼·자동 스크롤 토글(`⌘J`) | `ChatLiveScrollPolicy`, `ChatViewModel+ReadDivider` | `read_divider.rs`; 하단 근처면 자동 스크롤만 | S |
| CHT-12 | ✅ 채팅 지우기(`⇧⌘K`)·다시 연결 버튼 | `CViewApp` 채팅 메뉴, `ChatConnectionStatusBadge` | 상태 텍스트만 | S |
| CHT-13 | ✅ 채팅 디자인 스타일 8종(unified/chzzk/hybrid/kakao/imessage/calm/compact/dashboard) | `ChatCHZZKDesign`, `ChatHybridDesign`, `ChatMessengerDesign`, `ChatPanelView+Lounge`, `ChatMessageRow+*` | 단일 스타일 | L |
| CHT-14 | ✅ 표시 옵션: 글자 크기·줄 간격·타임스탬프·배지·이모티콘·정렬·읽기 폭 | `ChatSettingsTab`, `ChatReadingWidthMenu`, `ChatRowMetrics` | 설정 `chat` 도메인은 저장 가능, UI·렌더 미반영 | M |
| CHT-15 | 🟡 채팅 배경(채널 색/사용자 색/중립, 강도) | `ChatBackgroundScope`, `CViewUI/ChatBackgroundPalette`, `Components/ChatBackgroundSettingsRows` | 없음 | S |
| CHT-16 | ✅ 후원만 보기·후원 표시 끄기 | 설정 `showDonation`/`showDonationsOnly` | 없음 | S |
| CHT-17 | 🟡 후원·구독 TTS(음량·속도·음성) | `ChatTTSService` | `TtsQueue`(NoopTts) — Web Speech API 로 구현 가능 | M |
| CHT-18 | ✅ 채팅 이벤트 레일·하이브리드 이벤트 선반 | `ChatPanelEventRail`, `ChatHybridEventShelf` | 후원 랭킹 칩 줄만 | M |
| CHT-19 | ✅ 채팅 내보내기(텍스트 저장) | `ChatPanelView`("채팅 내보내기"), `/export` | 없음 | S |
| CHT-20 | ✅ 입력 상태 배너(로그인 필요·슬로우 모드·제한 사유) | `ChatInputStatusBanner`, `ChatRestrictionReason` | placeholder 문구만 | S |
| CHT-21 | 🟡 채팅 화면 갱신 상한·최대 표시 줄 수 | `ChatSettingsQualityView`, `maxUpdateRate`/`maxVisibleMessages` | `pacing.rs`, UI 는 300줄 고정 | S |
| CHT-22 | ✅ 채팅만 보기 화면(영상 없이) | `ChatOnlyView`, 라우트 `chatOnly` | 없음 | S |
| CHT-23 | 🧪 독립 채팅 창(`⇧⌘C`, 채널 피커) | `IndependentChatWindow`, `ChatChannelPickerView` | 없음 | M |
| CHT-24 | ✅ 채팅 통계(분당 메시지·참여자) | `ChatViewModel+Stats`, `Statistics/Tabs/ChatStatsView` | `stats.rs` | S |

## 5. 멀티채팅·멀티라이브 (MUL)

| ID | 항목 | Swift 원본 | Rust 현황 | 크기 |
|---|---|---|---|---|
| MUL-01 | ✅ 멀티채팅 화면(여러 채널 채팅만, 탭/그리드, 채널 추가·닫기) | `MultiChatView`, `MCTabBar*`, `MCTabChip`, `MultiChatGridLayouts`, `MultiChatTopChrome` | `MultiChatWorkspaceStore`, `MultiChatPreferences`; 채팅 엔진은 단일 세션(`chat_connect`) | L |
| MUL-02 | ✅ 통합(머지) 채팅(여러 채널 메시지 한 타임라인, 보낼 채널 선택) | `MergedChatView*` | `merged_chat_merge_state.rs`, `merged_budget.rs` | L |
| MUL-03 | ✅ 멀티채팅 메시지 검색(`⌥⌘F`)·다음/이전 채널(`⌃⇥`) | `MultiChatSearchView`, `MultiChatSearchStore` | `MultiChatSearchStore` | M |
| MUL-04 | ✅ 멀티채팅 개요·컨트롤 레일·알림 배너·오프라인 세션 정책 | `MultiChatOverviewView`, `MultiChatControlRail`, `MultiChatNoticeBanner` | `OfflineSessionPolicy` | M |
| MUL-05 | 🧪 멀티채팅 독립 창(`⇧⌘M`) | `MultiChatWindowHost` | 없음 | M |
| MUL-06 | ✅ 멀티라이브 레이아웃: 프리셋/사용자 배치(드래그 크기 조절), `⌃⌘1~3` | `MultiLiveGridLayouts`, `PaneResizeGrip`, `MCGridResizeDivider`, `MultiLiveLayoutModePolicy` | 그리드·포커스만(포커스 비율 드래그 있음) | M |
| MUL-07 | ✅ 패인 순서 드래그 재배치·탭 바 | `MLTabReorderDropDelegate`, `MCTabBar` | `order` 계산만, 드래그 없음 | S |
| MUL-08 | 🟡 패인 개별 컨트롤(볼륨·음소거·화질·재연결·채널 상세) | `MultiLivePaneOverlays`, `MLPlaybackVolumePanel` | 선택·닫기만 | M |
| MUL-09 | 🟡 다중 오디오(보조 패인 볼륨)·오디오 글로우 | `MultiLiveAudioGlow`, 설정 `multiAudioEnabled`/`secondaryVolume` | 선택 패인만 소리 | S |
| MUL-10 | 🟡 그리드 채팅 오버레이(투명도·글자 크기) | 설정 `chatOverlayInGrid` 등 | 사이드 채팅만 | M |
| MUL-11 | ✅ 싱글 채널 스테이지(멀티에서 한 채널 크게) | `MLSingleChannelStage` | 포커스 레이아웃으로 대체 | S |
| MUL-12 | ✅ 모든 스트림 재연결·일괄 해제 | `MultiLiveOverlays` | "모두 닫기"만 | S |
| MUL-13 | 🟡 멀티라이브 제안 토스트(자주 같이 보는 채널) | `MultiLiveProposerToast`, `MultiLiveProposer` | `MultiLiveProposer` | S |
| MUL-14 | ✅ 채널 프리셋 저장/불러오기(멀티 구성 묶음) | `ChannelPresetPicker`, `Components/SidebarPresetSaveSheet` | `ChannelPresetStore` | S |
| MUL-15 | ✅ 마지막 멀티 레이아웃·세션 기억 | `MultiLiveManager+Persistence`, 설정 `rememberLastLayout` | 미연결 | S |
| MUL-16 | ✅ 멀티라이브 설정 탭(최대 세션 수, 기본 레이아웃·방향, 대역폭 분배, 화면 크기 화질 캡) | `MultiLiveSettingsTab`, `MultiLiveSettingsPanel` | Rust `MultiLiveSettings` 모델, 설정 명령에 도메인 없음 | S |

## 6. 설정 (SET)

Swift 설정은 6그룹(개요/시청/채팅/멀티/연동/고급) + 검색 색인. Rust 는 `player`·`general`·`appearance`·`chat` 4도메인만 `settings_update` 가 받고, 화면에는 아래 9개만 있다: `lowLatencyMode`, `forceHighestQuality`, `startMuted`, `streamProxyMode`, `autoRefreshEnabled`, `notificationsEnabled`, `theme`, (로컬) 스트림 알림.

| ID | 항목 | Swift 원본 | Rust 현황 | 크기 |
|---|---|---|---|---|
| SET-01 | ✅ 설정 화면 구조: 그룹 탭 + 개요 + 설정 검색 | `SettingsView`, `SettingsWorkspace`, `SettingsOverviewContent`, `SettingsSearchIndex` | 한 페이지 목록 | M |
| SET-02 | ✅ 플레이어 키: `quality`, `autoPlay`, `catchupRate`, `bufferDuration`, `volumeLevel`, `continuePlaybackInBackground` | `PlayerSettingsTab` | 모델 있음, UI 없음 | S |
| SET-03 | ✅ 레이턴시 프리셋·고급(`latencyPreset/Target/Max/Min/MaxRate/MinRate/CatchUp/SlowDown`, PID `Kp/Ki/Kd`) | `LatencySettingsContent` | 모델·저지연 컨트롤러 있음, UI 없음 | M |
| SET-04 | ✅ 라이브 되감기 키(`liveRewindEnabled/WindowMinutes/DiskBudgetMB`), 실험 키(`experimentalServerTimeMachine`, `experimentalMatchWebV2`) | `PlayerSettingsTab` "라이브 되감기"·"고급 / 실험" | 모델 있음 | S |
| SET-05 | ✅ 스크린샷 키(`screenshotPath`, `screenshotFormat`) | `PlayerSettingsTab` "스크린샷" | 모델 있음 | S |
| SET-06 | ✅ 비디오·오디오 키(`equalizer*`, `videoAdjust*`, `sharpPixelScaling`, `autoPictureEnhancement`, `videoUpscaling`, `aspectRatio`) | `PlayerSettingsTab` "화면 & 비디오 보정"·"이퀄라이저" | 모델 있음(PLY-05~08 과 묶음) | S |
| SET-07 | 🟡 채팅 설정 탭(디자인·표시 모드·필터·스크롤·오버레이·TTS·배경·사용자 관리) | `ChatSettingsTab`, `Components/ChatSettingsSharedSections` | `chat` 도메인 저장만 | M |
| SET-08 | ✅ 일반 키: `launchAtLogin`, `showInMenuBar`, `alwaysOnTop`, `restoreWindowOnLaunch`, `autoRefreshInterval`, `notifyOnLiveStart/CategoryChange/TitleChange` | `GeneralSettingsTab` | 모델 있음, 2개만 노출 | S |
| SET-09 | ✅ 외관 키: `transparencyMode`, `motionQuality`, `frameRateMode`, `compactMode`, `sidebarWidth/Scale`, 테마 미리보기 | `GeneralSettingsTab`, `GeneralSettingsTab+ThemePreview` | 테마만 | S |
| SET-10 | 🟡 성능 탭(`maxMemoryMB`, `debugMode`, 하드웨어·메모리, 캐시 & 초기화, 로컬 진단 로그) | `PerformanceSettingsTab` | 없음 | M |
| SET-11 | ✅ 네트워크 탭(프리셋 balanced/stability/lowLatency/performance/custom, 요청 한도·동시성·타임아웃·재시도·재연결·`forceStreamProxy`·오프라인 대기) | `NetworkSettingsTab`, `NetworkPreset` | `NetworkSettings` 모델, 도메인 없음 | M |
| SET-12 | 🧪 키보드 단축키 편집(`KeyboardShortcutSettings` 바인딩) | `GeneralSettingsTab` "키보드 단축키" | 모델 있음 | M |
| SET-13 | 🧪 채널별 알림 설정(방송 시작/카테고리/제목 변경) | `ChannelNotificationSettings`, 채널 상세 | 모델 있음 | S |
| SET-14 | 🧪 치지직 OAuth 연동(클라이언트 ID/Secret 입력, 연결 상태) | `OAuthCredentialsSection`, `CViewAuth/ChzzkOAuthService` | `cview-auth` OAuth 이식, UI 없음(쿠키 로그인만) | M |
| SET-15 | 🟡 시청 기록 동기화(`SyncSettings` opt-in, 엔드포인트) | `AppState+Sync`, `WatchHistorySyncService` | `sync/watch_history_sync.rs` | S |
| SET-16 | ✅ `settings_update` 도메인 확장(network/multiLive/multiChat/keyboard/channelNotifications/sync) | — | 4도메인만 | S |
| SET-17 | ✅ 설정 백업·복원/초기화 | `SettingsStore+SnapshotRestore`, `SettingsSnapshotFile` | persistence `settings_snapshot_file`, `store_backup` | S |

## 7. 개인 데이터·통계 (DAT)

| ID | 항목 | Swift 원본 | Rust 현황 | 크기 |
|---|---|---|---|---|
| DAT-01 | ✅ 채널 메모(작성·편집 시트) | `ChannelMemoSheet` | persistence `fetch_memo/save_memo`, 명령 없음 | S |
| DAT-02 | ✅ 즐겨찾기(카드·채널 상세에서 토글, 사이드바 우선 표시) | `FavoritesStore`, `LiveCardActionMenu` | `FavoritesStore`, persistence | S |
| DAT-03 | ✅ 최근 시청을 DB 로 이관(지금은 localStorage) | `RecentChannelsStore` | `RecentChannelsStore` | S |
| DAT-04 | ✅ 통계/인사이트 화면(세션·스트리밍·채팅·기록 탭, KPI 카드) | `Statistics/StatisticsView`, `Tabs/*`, `Components/KPICard` | persistence `aggregate_insights`, `watch_time_by_channel/category`, `current_watch_streak` | L |
| DAT-05 | 🧪 통계 독립 창(`⇧⌘T`) | `CViewApp` `statistics-window` | 없음 | S |
| DAT-06 | 🟡 저장소 복구·로컬 데이터 정리 UI | `AppState+StoreRecovery`, `AppState+LocalDataHygiene` | `store_open_health`, `maintenance` | M |

## 8. 앱 셸·시스템 연동 (SYS)

| ID | 항목 | Swift 원본 | Rust 현황 | 크기 |
|---|---|---|---|---|
| SYS-01 | ✅ 전역 단축키 체계(재생 Space/M/↑↓, `⌘1~4`, `⌘R` 새로고침, `⌘,` 설정, `⌘F` 검색) | `CViewApp` Commands, `KeyboardShortcutSettings` | `⌘K`·`⌘\`·Esc 만 | M |
| SYS-02 | ✅ 키보드 단축키 도움말(`⌘/`) | `KeyboardShortcutsHelpView` | 없음 | S |
| SYS-03 | ✅ 앱 메뉴 바(재생/채팅/윈도우/도움말 메뉴) | `CViewApp` `CommandMenu` | Tauri 기본 메뉴 | M |
| SYS-04 | 🧪 팔로우 채널 방송 시작·카테고리·제목 변경 알림 | `NotificationService*`, `BackgroundUpdateService` | `BackgroundUpdateService` diff + `NotificationService`(Noop), `tauri-plugin-notification` 미도입 | M |
| SYS-05 | 🧪 멘션 알림 | `MentionNotificationService` | `MentionNotificationService` | S |
| SYS-06 | 🧪 딥 링크 `cview://live/…`·`channel`·`following`·`multilive`·`search`·`clip`·`login` | `DeepLinkRouter`, `DeepLinkInbox` | `deep_link.rs`·`deep_link_inbox.rs`, `tauri-plugin-deep-link` 미도입 | S |
| SYS-07 | 🧪 단일 인스턴스(두 번째 실행 시 기존 창 활성화·링크 전달) | `SingleInstanceGuard` | `SingleInstanceGuard` 로직, `tauri-plugin-single-instance` 미도입 | S |
| SYS-08 | 🧪 업데이트 확인 시트(GitHub 릴리스) | `UpdateSheetView`, `UpdateService` | `UpdateService`(GitHub 릴리스 파싱) | S |
| SYS-09 | ✅ 정보(About) 창·오픈소스 고지 | `AboutPanelView` | 없음 | S |
| SYS-10 | 🧪 스플래시·부트 진행 표시 | `SplashView`, `SplashContentView`, `BootProgress` | `BootProgress`, 없음 | S |
| SYS-11 | 🧪 환영 안내(첫 실행 온보딩, 다시 보기) | `Components/WelcomeOverlay` | 없음 | S |
| SYS-12 | ✅ 오프라인 배너·네트워크 상태 감지 | `Components/OfflineBanner`, `NetworkPathMonitor` | `NetworkPathMonitor` | S |
| SYS-13 | ✅ 토스트 센터(쌓임·액션 버튼) | `Components/ToastCenter` | 단일 토스트 | S |
| SYS-14 | ✅ 스켈레톤 로딩·신선도 캡션(마지막 갱신 시각) | `SkeletonViews`, `Components/FreshnessCaption` | 스피너 | S |
| SYS-15 | ✅ 자동 새로고침 주기 적용(`autoRefreshInterval`) | `RefreshCoordinator` | `RefreshCoordinator`, 사이드바만 120초 고정 | S |
| SYS-16 | ✅ 창 위치·크기 저장/복원 | `WindowFrameAutosave`, `restoreWindowOnLaunch` | 없음(`tauri-plugin-window-state` 로 대체 가능) | S |
| SYS-17 | ✅ 항상 위(`alwaysOnTop`) | `GeneralSettings.alwaysOnTop` | 없음(`set_always_on_top`) | S |
| SYS-18 | ✅ 로그인 시 자동 실행 | `launchAtLogin` | 없음(`tauri-plugin-autostart`) | S |
| SYS-19 | 🟡 도움말 액션(문제 보고, 알림 설정 열기, 로그 폴더 열기) | `SupportActions`, `SystemSettingsActions` | 없음 | S |
| SYS-20 | 🧪 메뉴 막대 앱(방송 중 팔로잉 목록, 열기/종료) | `MenuBarView`, `MenuBar/*` | 없음(Tauri 트레이 아이콘으로 근사) | M |
| SYS-21 | 🧪 화면 속 화면(PiP, `⌥⌘P`) | `PiPController`, `AppState+PiP` | 없음(WebView `requestPictureInPicture`) | M |
| SYS-22 | 🧪 창 투명도/Liquid Glass·트래픽 라이트 애니메이션 | `DesignSystem/LiquidGlass`, `TransparencyPolicy`, `TrafficLightsAnimator` | 없음 | M |
| SYS-23 | 🧪 앱 아이콘 동적 렌더(라이브 수 배지 등) | `AppIconRenderer`, `Components/AppIconView` | 없음 | S |
| SYS-24 | ⏸ 위젯(WidgetKit) | `AppState+Widget`, `WidgetSnapshotWriter`, `CViewWidgets` | 없음 | L |
| SYS-25 | ⏸ App Intents(단축어/Siri) | `Intents/CViewIntents` | 없음 | M |
| SYS-26 | 🧪 서명·공증·배포 스크립트 | 공증 스크립트 | Tauri 번들 설정 미작성 | S |
| SYS-27 | ⏸ 인증서 피닝 | `CertificatePinningDelegate` | 제외됨 | M |

## 9. 디자인 (DSN)

| ID | 항목 | Swift 원본 | Rust 현황 | 크기 |
|---|---|---|---|---|
| DSN-01 | 🧪 디자인 토큰 정합(간격·반경·의미 색·카테고리 색) | `DesignSystem/DesignTokens*` | `css/app.css` 자체 토큰 | M |
| DSN-02 | ✅ 모션 등급(reduced/balanced/rich)·화면 전환 애니메이션 | `CVAnimation`, `RouteTransitionTiming`, `MotionTransactionPolicy` | `prefers-reduced-motion` 만 | S |
| DSN-03 | ✅ 콤팩트 밀도 모드 | `CompactDensity`, `compactMode` | 없음 | S |
| DSN-04 | 🧪 채널 프로필 톤(채널 색 강조) | `ChannelProfileTone`, `ChannelIdentity` | 없음 | S |
| DSN-05 | ✅ 차트·랭킹 바 스타일(통계용) | `CVChartStyle`, `CVRankingBar` | 없음(DAT-04 와 묶음) | S |
| DSN-06 | 🧪 접근성 레이블·키보드 포커스 이동 정합 | `Components/CardAccessibility`, `CViewCore/Focus` | 일부 aria | S |

---

## 권장 구현 순서

### P1 — 사용자가 가장 먼저 체감하는 시청·채팅·탐색 (45개)

1. **시청 화면 기본기**: PLY-01 화질 선택기, PLY-02 일시정지·라이브 복귀, PLY-11 전체화면 2종, PLY-15 볼륨, PLY-13 오버레이, PLY-20 시청 기록, PLY-09 스크린샷.
2. **채팅 필수**: CHT-01 이모티콘 선택기, CHT-02 자동완성, CHT-03 컨텍스트 메뉴, CHT-05 차단, CHT-06 키워드 필터, CHT-11 스크롤·읽음 구분선, CHT-12 지우기·재연결, CHT-14 표시 옵션, CHT-20 입력 상태 배너, CHT-08 멘션 트레이, CHT-10 핀/공지, CHT-04 프로필 시트.
3. **탐색**: NAV-01 채널 상세, NAV-06 통합 검색, NAV-02 카테고리, NAV-04 팔로잉 허브, NAV-03 인기 고도화, NAV-10 카드 메뉴, NAV-11 뒤로 가기, NAV-12 커맨드 팔레트, NAV-09 최근·즐겨찾기, NAV-14 홈 보강, DAT-02 즐겨찾기, DAT-03 최근 시청 DB.
4. **VOD·클립**: VOD-03 클립 플레이어, VOD-04 인기 클립, VOD-05 채널 VOD·클립 탭, VOD-01 VOD 목록, VOD-02 VOD 플레이어(WebView `<video>`+hls.js 엔진 구현 포함), NAV-08 라이브러리 허브.
5. **단축키·셸**: SYS-01 전역 단축키, SYS-02 도움말, SYS-03 앱 메뉴, SYS-12 오프라인 배너, SYS-13 토스트 센터, SYS-14 스켈레톤.
6. **멀티라이브 핵심 보강**: MUL-08 패인 컨트롤, MUL-12 일괄 재연결.

### P2 — 설정·관리·고급 기능 (78개)

- 설정: SET-01 ~ SET-17 전부(먼저 SET-16 도메인 확장 → SET-01 구조 → 각 탭).
- 플레이어 고급: PLY-03 되감기(로컬 타임시프트 녹화기 등록 포함), PLY-04 속도, PLY-05 고급 패널, PLY-06 이퀄라이저, PLY-07 비디오 보정, PLY-08 화면 비율, PLY-10 녹화, PLY-12 채팅 오버레이 모드, PLY-14 헤더 채널 레일, PLY-16 알림 위치, PLY-17 북마크, PLY-19 백그라운드 정책.
- 채팅 확장: CHT-07 명령어, CHT-09 하이라이트, CHT-13 디자인 스타일 8종, CHT-15 배경, CHT-16 후원 필터, CHT-17 TTS(Web Speech API), CHT-18 이벤트 레일, CHT-19 내보내기, CHT-21 갱신 상한, CHT-22 채팅만 보기, CHT-24 채팅 통계.
- 멀티: MUL-01 멀티채팅(채팅 엔진 다중 세션화가 선행 — `chat_connect` 가 단일 세션), MUL-02 머지 채팅, MUL-03 검색, MUL-04 개요, MUL-06 레이아웃, MUL-07 재배치, MUL-09 다중 오디오, MUL-10 그리드 채팅 오버레이, MUL-11 싱글 스테이지, MUL-13 제안 토스트, MUL-14 프리셋, MUL-15 레이아웃 기억, MUL-16 멀티 설정.
- 데이터: DAT-01 메모, DAT-04 통계, DAT-06 저장소 복구, VOD-06 다운로더, VOD-07 다운로드 보관함.
- 탐색 부가: NAV-05 인사이트 레일, NAV-07 디스커버리, NAV-13 사이드바 빠른 액션, NAV-15 허브 모드, NAV-16 워크스페이스 복원.
- 시스템(크로스플랫폼으로 가능한 것): SYS-04 방송 시작 알림(`tauri-plugin-notification`), SYS-05 멘션 알림, SYS-06 딥 링크, SYS-07 단일 인스턴스, SYS-08 업데이트 확인(`tauri-plugin-updater` 로 대체 권장), SYS-15 자동 새로고침 주기, SYS-16 창 상태, SYS-17 항상 위, SYS-18 자동 실행.
- 디자인: DSN-01 ~ DSN-06.

### P3 — macOS 전용·선택 사항 (16개)

- **Tauri 로 가능(근사)**: SYS-20 메뉴 막대 → 트레이 아이콘+메뉴(팔로잉 목록), PLY-18/CHT-23/MUL-05/DAT-05 보조 창(Tauri 다중 `WebviewWindow` — 채팅·플레이어 상태를 창 간 이벤트로 공유해야 함), SYS-09 About, SYS-10 스플래시, SYS-11 환영 안내, SYS-19 도움말 액션(`tauri-plugin-opener`), SYS-23 아이콘 배지(macOS Dock 배지 정도만), SYS-26 서명·공증(Tauri 번들러가 `APPLE_*` 환경 변수로 지원 — 설정만 하면 됨).
- **부분 가능·제약 있음**:
  - SYS-21 PiP — Windows WebView2(Chromium)는 `requestPictureInPicture` 지원. macOS WKWebView 는 iframe 안 `<video>` 의 PiP 를 막는 경우가 있어 실기 확인 필요. AVKit 수준(창 크기·복귀 동작) 동등성은 불가.
  - SYS-22 투명도/Liquid Glass — macOS 는 `window-vibrancy` 크레이트로 블러 배경 근사, Windows 는 Mica/Acrylic. Liquid Glass 재질 자체와 트래픽 라이트 애니메이션은 불가.
- **불가 또는 보류**:
  - SYS-24 위젯 — WidgetKit 확장은 Swift 타깃이어야 한다. 남긴다면 Rust 가 스냅숏 JSON 을 쓰고 작은 Swift 위젯 타깃이 읽는 하이브리드뿐 → 보류.
  - SYS-25 App Intents — Swift 전용. 딥 링크(SYS-06)로 대부분 대체 → 보류.
  - SYS-27 인증서 피닝 — `reqwest`+rustls 커스텀 검증기로 가능하지만 WebView 쪽 요청(hls.js 직접 재생 시)은 피닝 불가. 로컬 프록시 경유 시만 효과 → 보류.

### 티어별 개수

| 티어 | 개수 |
|---|---|
| P1 | 45 |
| P2 | 78 |
| P3 | 16 |
| 합계 | 139 |

(보조 창 계열 PLY-18·CHT-23·MUL-05·DAT-05 는 P3 로 셌다. 각 ID 는 한 티어에만 들어 있다.)

## 선행 작업(여러 항목이 기대는 것)

1. ✅ **명령 계층 확장**: 채널 상세·VOD·클립·카테고리·즐겨찾기·메모·북마크·차단 목록 명령. 대부분 app-core 뷰모델이 이미 있어 DTO+명령만 붙이면 된다.
2. ✅ **채팅 엔진 다중 세션화** (세션 키별 ChatViewModel, `chat://session-event`): 지금 `ChatViewModel` 하나를 셸이 쥐고 있다. 멀티라이브 채팅, 멀티채팅, 머지 채팅, 독립 채팅 창이 모두 세션 ID 기반 명령/이벤트를 요구한다.
3. ✅ **파일 저장 경로** (`save_file`·`reveal_path`, 녹화는 StreamRecordingService): 스크린샷·녹화·채팅 내보내기·다운로드 모두 저장 대화상자(`tauri-plugin-dialog`)와 쓰기 명령이 필요하다.
4. ✅ **VOD 재생 엔진** (WebView 기본 HLS `<video>`, 클립은 임베드): `VodPlayerEngine` 트레잇의 WebView 구현(player.html 의 VOD 모드 또는 별도 vod.html).
5. ✅ **라우터 도입** (화면 파라미터·뒤로 가기 스택): 지금 `app.js` 의 라우트는 평면 목록이다. 파라미터 있는 라우트(`channel/:id`, `vod/:no`, `clip/:uid`)와 뒤로 가기 스택이 필요하다.
