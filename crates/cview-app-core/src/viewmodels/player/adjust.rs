//! 오디오·비디오 보정 — 이퀄라이저·영상 조정·화면 비율 (`PlayerViewModel+AudioVideoAdjust`).

use cview_core::models::equalizer::EqualizerPreset;
use cview_core::models::settings::PlayerSettings;

use super::{
    equalizer_bands_fitting_frequencies, PlayerState, PlayerViewModel, SharedLiveEngine,
    VideoAdjust,
};

impl PlayerViewModel {
    pub fn equalizer_presets(&self) -> Vec<&'static str> {
        if self.state.read().can_use_equalizer() {
            EqualizerPreset::names()
        } else {
            Vec::new()
        }
    }

    pub fn equalizer_frequencies(&self) -> Vec<f32> {
        if self.state.read().can_use_equalizer() {
            EqualizerPreset::FREQUENCIES.to_vec()
        } else {
            Vec::new()
        }
    }

    pub fn apply_equalizer_preset(&self, name: &str) {
        let Some(preset) = EqualizerPreset::preset_named(name) else {
            return;
        };
        let mut s = self.state.lock();
        let Some(engine) = s.engine.clone().filter(|e| e.is_hlsjs()) else {
            return;
        };
        s.equalizer_preset_name = name.to_string();
        s.is_equalizer_enabled = true;
        s.equalizer_pre_amp = preset.pre_amp;
        s.equalizer_bands = preset.gains.to_vec();
        engine.set_equalizer_enabled(true, &preset.gains, preset.pre_amp);
    }

    fn enable_equalizer_graph(s: &mut PlayerState, engine: &SharedLiveEngine) {
        if s.equalizer_bands.is_empty() {
            s.equalizer_bands = vec![0.0; EqualizerPreset::FREQUENCIES.len()];
        }
        s.is_equalizer_enabled = true;
        engine.set_equalizer_enabled(true, &s.equalizer_bands, s.equalizer_pre_amp);
    }

    fn refresh_preset_name(s: &mut PlayerState) {
        s.equalizer_preset_name =
            EqualizerPreset::matching(&s.equalizer_bands, s.equalizer_pre_amp)
                .map(|p| p.name.to_string())
                .unwrap_or_default();
    }

    pub fn set_equalizer_pre_amp(&self, value: f32) {
        let mut s = self.state.lock();
        s.equalizer_pre_amp = value;
        let Some(engine) = s.engine.clone().filter(|e| e.is_hlsjs()) else {
            return;
        };
        if !s.is_equalizer_enabled || s.equalizer_bands.is_empty() {
            Self::enable_equalizer_graph(&mut s, &engine);
        }
        engine.set_equalizer_pre_amp(value);
        Self::refresh_preset_name(&mut s);
    }

    pub fn set_equalizer_band(&self, index: usize, value: f32) {
        let mut s = self.state.lock();
        let had_no_graph = s.equalizer_bands.is_empty();
        if had_no_graph {
            s.equalizer_bands = vec![0.0; EqualizerPreset::FREQUENCIES.len()];
        }
        if index >= s.equalizer_bands.len() {
            return;
        }
        s.equalizer_bands[index] = value;
        let Some(engine) = s.engine.clone().filter(|e| e.is_hlsjs()) else {
            return;
        };
        if !s.is_equalizer_enabled || had_no_graph {
            Self::enable_equalizer_graph(&mut s, &engine);
        }
        engine.set_equalizer_band(index, value);
        Self::refresh_preset_name(&mut s);
    }

    pub fn disable_equalizer(&self) {
        let mut s = self.state.lock();
        s.is_equalizer_enabled = false;
        s.equalizer_preset_name.clear();
        s.equalizer_pre_amp = 0.0;
        s.equalizer_bands.clear();
        if let Some(e) = s.engine.as_ref().filter(|e| e.is_hlsjs()) {
            e.set_equalizer_enabled(false, &[], 0.0);
        }
    }

    fn push_video_adjust(s: &PlayerState) {
        let Some(engine) = s.engine.as_ref().filter(|e| e.is_hlsjs()) else {
            return;
        };
        engine.apply_video_adjust(VideoAdjust {
            enabled: s.is_video_adjust_enabled,
            brightness: s.video_brightness as f64,
            contrast: s.video_contrast as f64,
            saturation: s.video_saturation as f64,
            hue: s.video_hue as f64,
            gamma: s.video_gamma as f64,
            sharp_pixel: engine.sharp_pixel_scaling_enabled(),
        });
    }

    pub fn set_video_adjust_enabled(&self, enabled: bool) {
        let mut s = self.state.lock();
        s.is_video_adjust_enabled = enabled;
        Self::push_video_adjust(&s);
    }

    pub fn set_video_brightness(&self, v: f32) {
        let mut s = self.state.lock();
        s.video_brightness = v;
        Self::push_video_adjust(&s);
    }

    pub fn set_video_contrast(&self, v: f32) {
        let mut s = self.state.lock();
        s.video_contrast = v;
        Self::push_video_adjust(&s);
    }

    pub fn set_video_saturation(&self, v: f32) {
        let mut s = self.state.lock();
        s.video_saturation = v;
        Self::push_video_adjust(&s);
    }

    pub fn set_video_hue(&self, v: f32) {
        let mut s = self.state.lock();
        s.video_hue = v;
        Self::push_video_adjust(&s);
    }

    pub fn set_video_gamma(&self, v: f32) {
        let mut s = self.state.lock();
        s.video_gamma = v;
        Self::push_video_adjust(&s);
    }

    pub fn reset_video_adjust(&self) {
        let mut s = self.state.lock();
        s.is_video_adjust_enabled = false;
        s.video_brightness = 1.0;
        s.video_contrast = 1.0;
        s.video_saturation = 1.0;
        s.video_hue = 0.0;
        s.video_gamma = 1.0;
        Self::push_video_adjust(&s);
    }

    pub fn set_aspect_ratio(&self, ratio: Option<String>) {
        self.state.lock().aspect_ratio = ratio;
    }

    /// 고급 설정(화면비·스크린샷·비디오 보정·EQ) 복원.
    pub fn apply_advanced_settings(&self, settings: &PlayerSettings) {
        let preset_to_apply = {
            let mut s = self.state.lock();
            s.aspect_ratio = settings.aspect_ratio.clone();
            s.screenshot_save_path = settings.screenshot_path.clone();
            s.screenshot_format = settings.screenshot_format;
            s.is_video_adjust_enabled = settings.video_adjust_enabled;
            s.video_brightness = settings.video_brightness;
            s.video_contrast = settings.video_contrast;
            s.video_saturation = settings.video_saturation;
            s.video_hue = settings.video_hue;
            s.video_gamma = settings.video_gamma;

            let has_stored =
                !settings.equalizer_bands.is_empty() || settings.equalizer_pre_amp != 0.0;
            let stored_bands = equalizer_bands_fitting_frequencies(&settings.equalizer_bands);
            if has_stored {
                s.is_equalizer_enabled = true;
                s.equalizer_pre_amp = settings.equalizer_pre_amp;
                s.equalizer_bands = stored_bands.clone();
                Self::refresh_preset_name(&mut s);
            }
            let Some(engine) = s.engine.clone().filter(|e| e.is_hlsjs()) else {
                return;
            };
            engine.apply_video_adjust(VideoAdjust {
                enabled: settings.video_adjust_enabled,
                brightness: settings.video_brightness as f64,
                contrast: settings.video_contrast as f64,
                saturation: settings.video_saturation as f64,
                hue: settings.video_hue as f64,
                gamma: settings.video_gamma as f64,
                sharp_pixel: settings.sharp_pixel_scaling,
            });
            if has_stored {
                engine.set_equalizer_enabled(true, &stored_bands, settings.equalizer_pre_amp);
                None
            } else {
                settings.equalizer_preset.clone().filter(|p| !p.is_empty())
            }
        };
        if let Some(name) = preset_to_apply {
            self.apply_equalizer_preset(&name);
        }
    }
}
