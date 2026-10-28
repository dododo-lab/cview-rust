//! 스트림 시작/정지 (`PlayerViewModel+StreamStart`).

use cview_core::models::player::PlayerEngineType;
use cview_core::models::settings::ExperimentalPlaybackFlags;
use cview_player::coordinator::{
    Configuration, LowLatencyActivation, StreamCoordinator, StreamPhase,
};
use cview_player::hlsjs::HlsJsMatchWebV2Config;
use cview_player::policy::LatencyPresetPairing;
use cview_player::LowLatencyConfiguration;
use url::Url;

use super::{apply_quality_intent, upcast, PlayerViewModel, RewindSettings, StartStreamRequest};
use crate::logic::effective_playback_policy::EffectivePlaybackPolicy;
use crate::viewmodels::support::cancel_slot;

impl PlayerViewModel {
    /// 스트림 시작. 앞선 시작은 끊고, 정지와 교차하면 결과를 버린다.
    pub async fn start_stream(&self, request: StartStreamRequest) {
        let generation = {
            let mut s = self.state.lock();
            s.start_generation = s.start_generation.wrapping_add(1);
            s.start_generation
        };
        let _guard = self.start_lock.lock().await;
        if self.state.read().start_generation != generation {
            return;
        }
        self.perform_start_stream(request, generation).await;
    }

    fn is_current_start(&self, generation: u64) -> bool {
        self.state.read().start_generation == generation
    }

    async fn perform_start_stream(&self, req: StartStreamRequest, generation: u64) {
        let settings = req.player_settings.clone().unwrap_or_default();
        let policy = EffectivePlaybackPolicy::new(&settings);
        let (is_multi_live, time_machine, precision, multi_target) = {
            let mut s = self.state.lock();
            Self::initialize_session_volume_locked(&mut s, &settings);
            s.current_quality = None;
            s.available_qualities.clear();
            s.multi_live_source_qualities.clear();
            s.latency_info = None;
            s.latency_history.clear();
            s.latency_sample_sequence = 0;
            s.buffer_health = None;
            s.playback_rate = 1.0;
            s.error_message = None;
            s.is_time_shifting = false;
            s.channel_name = req.channel_name.clone();
            s.live_title = req.live_title.clone();
            s.thumbnail_url = req.thumbnail_url.clone();
            s.current_channel_id = Some(req.channel_id.clone());
            s.aspect_ratio = req
                .player_settings
                .as_ref()
                .and_then(|p| p.aspect_ratio.clone());
            s.proxy_route_status = None;
            s.current_proxy_mode = "pending".into();
            (
                s.is_multi_live,
                s.is_time_machine_available,
                s.precision_sync_requested,
                s.multi_live_quality_target,
            )
        };

        let match_web_v2 = LatencyPresetPairing::is_match_web_v2_eligible(
            req.player_settings.as_ref(),
            ExperimentalPlaybackFlags::match_web_v2(None),
            time_machine,
            req.match_web_v2_source_eligible,
        );
        let low_latency_config = if match_web_v2 {
            LowLatencyConfiguration::MATCH_WEB_V2_HLSJS
        } else {
            req.player_settings
                .as_ref()
                .map(|p| LatencyPresetPairing::pairing(Some(p)).configuration)
                .unwrap_or(LowLatencyConfiguration::WEB_PARITY_HLSJS)
        };
        let mut config = Configuration::new(req.channel_id.clone());
        config.enable_low_latency = policy.low_latency_enabled;
        config.low_latency_activation = if is_multi_live {
            LowLatencyActivation::Managed
        } else {
            LowLatencyActivation::Automatic
        };
        config.preferred_quality = policy.preferred_quality;
        config.low_latency_config = low_latency_config;
        config.force_highest_quality = policy.force_highest_quality();
        config.quality_policy = Some(policy.quality_policy);
        config.stream_proxy_mode = settings.stream_proxy_mode;

        let old = self.state.lock().coordinator.take();
        if let Some(old) = old {
            old.stop_stream().await;
        }
        if !self.is_current_start(generation) {
            return;
        }
        let coordinator = StreamCoordinator::new(config, req.playback_key_access.clone());
        let session = {
            let mut s = self.state.lock();
            s.session_id = s.session_id.wrapping_add(1);
            s.coordinator = Some(coordinator.clone());
            // 다시 연결·채널 바꾸기로 새 코디네이터가 생겨도 매니저가 정한 여유를 잃지 않는다
            if s.latency_margin > 0.0 {
                coordinator.set_target_latency_margin(s.latency_margin);
            }
            s.rewind_settings = RewindSettings::from_settings(&settings);
            Self::apply_timeshift_window(&mut s, 0.0);
            s.rewind_seek_hint = None;
            s.session_id
        };
        coordinator
            .set_server_time_machine_stream(time_machine)
            .await;
        let rewind_config = self.state.read().rewind_recording_config();
        coordinator.set_timeshift_recording(rewind_config).await;
        if is_multi_live && precision {
            coordinator.set_low_latency_sync_active(true).await;
        }
        if let Some(check) = self.callbacks().on_check_stream_ended {
            coordinator.set_check_stream_ended_callback(check);
        }
        if let Some(manifest) = req.prefetched_manifest.clone() {
            coordinator.set_prefetched_manifest(manifest);
        }

        // 가드를 `await` 너머로 들고 가지 않도록 엔진 확보와 실패 처리를 나눈다(Send 퓨처).
        let engine = {
            let mut s = self.state.lock();
            let engine = match (&s.engine, s.is_preallocated) {
                (Some(e), true) => Some(e.clone()),
                _ => self.engine_factory.as_ref().map(|factory| {
                    let e = factory();
                    s.engine = Some(e.clone());
                    e
                }),
            };
            if let Some(engine) = &engine {
                s.current_engine_type = PlayerEngineType::Hlsjs;
                engine.set_volume(if s.is_muted { 0.0 } else { s.volume });
            }
            engine
        };
        let Some(engine) = engine else {
            self.fail_start(&coordinator, "재생 엔진이 없습니다".into())
                .await;
            return;
        };

        if engine.is_hlsjs() {
            engine.set_streaming_profile(
                LatencyPresetPairing::pairing(req.player_settings.as_ref()).profile,
            );
            engine.set_match_web_v2_config(match_web_v2.then(|| {
                HlsJsMatchWebV2Config::new(
                    cview_core::models::match_web_v2::MatchWebV2Flag::resolve_ingest_target(None),
                )
            }));
            engine.set_match_web_v2_seek_priority(if !is_multi_live || precision { 1 } else { 0 });
            apply_quality_intent(engine.as_ref(), &policy, is_multi_live, multi_target);
            let tag: String = req.channel_id.chars().take(6).collect();
            engine.set_parity_tag(&tag);
        }
        coordinator.set_player_engine(upcast(&engine)).await;

        self.start_event_listening(&coordinator, session);
        self.start_configured_coordinator(
            coordinator,
            req.stream_url.clone(),
            rewind_config.is_some(),
            generation,
        )
        .await;
    }

    async fn fail_start(&self, coordinator: &StreamCoordinator, message: String) {
        coordinator.stop_stream().await;
        {
            let mut s = self.state.lock();
            cancel_slot(&mut s.event_task);
            s.coordinator = None;
            let msg = format!("스트림 시작 실패: {message}");
            s.error_message = Some(msg.clone());
            s.stream_phase = StreamPhase::Error(msg);
        }
        self.notify_playback_state_changed();
    }

    async fn start_configured_coordinator(
        &self,
        coordinator: StreamCoordinator,
        url: Url,
        records_rewind: bool,
        generation: u64,
    ) {
        let result = coordinator.start_stream(url).await;
        let route = coordinator.proxy_route_status();
        let still_current =
            self.is_current_session(&coordinator) && self.is_current_start(generation);
        match result {
            Ok(()) => {
                if !still_current {
                    coordinator.stop_stream().await;
                    return;
                }
                Self::apply_proxy_route_status(&mut self.state.lock(), route);
                self.start_uptime_timer(coordinator.clone());
                if records_rewind {
                    self.start_rewind_window_updates(coordinator);
                }
            }
            Err(e) => {
                if !still_current {
                    coordinator.stop_stream().await;
                    return;
                }
                Self::apply_proxy_route_status(&mut self.state.lock(), route);
                tracing::error!("스트림 시작 실패: {e}");
                self.fail_start(&coordinator, e.to_string()).await;
                // 실패한 세션의 프록시 상태는 남겨 진단에 쓴다.
                Self::apply_proxy_route_status(&mut self.state.lock(), route);
            }
        }
    }

    pub(super) fn is_current_session(&self, coordinator: &StreamCoordinator) -> bool {
        // 코디네이터는 Arc 핸들이다 — 같은 세션인지는 우리가 넣은 핸들의 설정 채널로 확인한다.
        self.state
            .read()
            .coordinator
            .as_ref()
            .is_some_and(|c| std::ptr::eq(c.configuration(), coordinator.configuration()))
    }

    /// 정지 — 시작 중이면 끊고, 녹화·타이머를 거두고, 엔진을 멈춘다.
    pub async fn stop_stream(&self) {
        {
            let mut s = self.state.lock();
            s.start_generation = s.start_generation.wrapping_add(1);
        }
        let _guard = self.start_lock.lock().await;
        if self.state.read().is_recording {
            self.stop_recording().await;
        }
        let coordinator = {
            let mut s = self.state.lock();
            cancel_slot(&mut s.recording_timer);
            cancel_slot(&mut s.uptime_task);
            cancel_slot(&mut s.event_task);
            cancel_slot(&mut s.rewind_window_task);
            cancel_slot(&mut s.rewind_release_task);
            Self::apply_timeshift_window(&mut s, 0.0);
            cancel_slot(&mut s.control_hide_task);
            cancel_slot(&mut s.buffering_debounce);
            s.last_playing_time = None;
            s.session_id = s.session_id.wrapping_add(1);
            s.coordinator.take()
        };
        if let Some(c) = coordinator {
            c.stop_stream().await;
        }
        {
            let mut s = self.state.lock();
            if s.is_preallocated {
                if let Some(e) = &s.engine {
                    e.stop();
                }
            } else if let Some(e) = s.engine.take() {
                e.stop();
            }
            Self::apply_proxy_route_status(&mut s, None);
            s.uptime = 0.0;
            s.stream_phase = StreamPhase::Idle;
            s.current_quality = None;
            s.available_qualities.clear();
            s.multi_live_source_qualities.clear();
            s.latency_info = None;
            s.latency_history.clear();
            s.latency_sample_sequence = 0;
            s.buffer_health = None;
            s.playback_rate = 1.0;
            s.is_cheat_key_user = false;
            s.is_time_machine_available = false;
            s.is_time_shifting = false;
        }
        self.notify_playback_state_changed();
    }

    pub async fn toggle_play_pause(&self) {
        let (coordinator, phase) = {
            let s = self.state.read();
            (s.coordinator.clone(), s.stream_phase.clone())
        };
        let Some(c) = coordinator else {
            return;
        };
        match phase {
            StreamPhase::Playing => c.pause().await,
            StreamPhase::Paused => c.resume().await,
            _ => {}
        }
    }
}
