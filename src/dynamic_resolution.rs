//! GPU-timing driven dynamic render-resolution control.
//!
//! The controller is deliberately independent of wgpu.  A renderer can feed
//! completed, frame-tagged timestamp results when they become available and
//! use the returned quantized scale to decide whether to recreate its target.

use std::time::Duration;

pub const SCALE_STEP: f32 = 1.0 / 16.0;
pub const MIN_SCALE: f32 = 0.5;
pub const MAX_SCALE: f32 = 1.0;
pub const HYSTERESIS: f32 = 0.10;
pub const ADJUSTMENT_COOLDOWN_FRAMES: u32 = 30;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DynamicResolutionConfig {
    /// Zero means no FPS cap; `target_hz` is then used (normally 60 Hz).
    pub fps_cap: u32,
    pub target_hz: f32,
    /// Scale used when timestamps are unsupported or unavailable.
    pub fallback_scale: f32,
    pub enabled: bool,
    pub gpu_timestamps_supported: bool,
}

impl Default for DynamicResolutionConfig {
    fn default() -> Self {
        Self {
            fps_cap: 0,
            target_hz: 60.0,
            fallback_scale: 1.0,
            enabled: true,
            gpu_timestamps_supported: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GpuFrameTiming {
    pub frame_tag: u64,
    pub total: Option<Duration>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderExtent {
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderTargetKey {
    pub extent: RenderExtent,
    pub scale: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct DynamicResolutionController {
    config: DynamicResolutionConfig,
    scale: f32,
    cooldown: u32,
    newest_frame_tag: Option<u64>,
}

impl DynamicResolutionController {
    pub fn new(config: DynamicResolutionConfig) -> Self {
        let fallback = quantize_scale(config.fallback_scale);
        let scale = if config.enabled { fallback } else { MAX_SCALE };
        Self {
            config,
            scale,
            cooldown: 0,
            newest_frame_tag: None,
        }
    }

    pub fn scale(&self) -> f32 {
        self.scale
    }
    pub fn cooldown_remaining(&self) -> u32 {
        self.cooldown
    }
    pub fn target_budget(&self) -> Duration {
        let hz = if self.config.fps_cap > 0 {
            self.config.fps_cap as f32
        } else {
            self.config.target_hz
        };
        let hz = if hz.is_finite() && hz > 0.0 { hz } else { 60.0 };
        Duration::from_secs_f32(1.0 / hz)
    }

    /// Submit a delayed result. Missing/unsupported timestamps and stale
    /// frame tags are ignored; no synthetic timing sample is generated.
    pub fn submit(&mut self, timing: GpuFrameTiming) -> f32 {
        if !self.config.enabled || !self.config.gpu_timestamps_supported {
            self.scale = if self.config.enabled {
                quantize_scale(self.config.fallback_scale)
            } else {
                MAX_SCALE
            };
            return self.scale;
        }
        if self
            .newest_frame_tag
            .is_some_and(|tag| timing.frame_tag <= tag)
        {
            return self.scale;
        }
        self.newest_frame_tag = Some(timing.frame_tag);
        let Some(total) = timing.total else {
            // No timestamp means this frame cannot provide feedback. Keep
            // behaviour deterministic by returning to the configured fixed
            // scale instead of fabricating a sample.
            self.scale = quantize_scale(self.config.fallback_scale);
            return self.scale;
        };
        if self.cooldown > 0 {
            self.cooldown -= 1;
            return self.scale;
        }

        let budget = self.target_budget().as_secs_f32();
        let elapsed = total.as_secs_f32();
        if elapsed > budget * (1.0 + HYSTERESIS) && self.scale > MIN_SCALE {
            self.scale = quantize_scale(self.scale - SCALE_STEP);
            self.cooldown = ADJUSTMENT_COOLDOWN_FRAMES;
        } else if elapsed < budget * (1.0 - HYSTERESIS) && self.scale < MAX_SCALE {
            self.scale = quantize_scale(self.scale + SCALE_STEP);
            self.cooldown = ADJUSTMENT_COOLDOWN_FRAMES;
        }
        self.scale
    }
}

pub fn quantize_scale(scale: f32) -> f32 {
    let value = if scale.is_finite() { scale } else { MAX_SCALE };
    ((value.clamp(MIN_SCALE, MAX_SCALE) * 16.0).round() / 16.0).clamp(MIN_SCALE, MAX_SCALE)
}

/// Compute a target extent. A minimized window yields zero dimensions and is
/// intentionally not turned into a 1x1 allocation.
pub fn scaled_extent(width: u32, height: u32, scale: f32) -> RenderExtent {
    if width == 0 || height == 0 {
        return RenderExtent {
            width: 0,
            height: 0,
        };
    }
    let scale = quantize_scale(scale);
    RenderExtent {
        width: ((width as f32 * scale).round() as u32).max(1),
        height: ((height as f32 * scale).round() as u32).max(1),
    }
}

/// Whether a render target must be recreated after a resize or scale change.
/// Minimized surfaces do not trigger a pointless zero-sized recreation.
pub fn should_recreate_target(
    current: Option<RenderTargetKey>,
    width: u32,
    height: u32,
    scale: f32,
) -> bool {
    if width == 0 || height == 0 {
        return false;
    }
    let desired_scale = quantize_scale(scale);
    let desired = scaled_extent(width, height, desired_scale);
    current.map_or(true, |key| {
        key.scale != desired_scale || key.extent != desired
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn timing(tag: u64, ms: u64) -> GpuFrameTiming {
        GpuFrameTiming {
            frame_tag: tag,
            total: Some(Duration::from_millis(ms)),
        }
    }

    #[test]
    fn quantization_and_bounds() {
        assert_eq!(quantize_scale(0.1), 0.5);
        assert_eq!(quantize_scale(1.4), 1.0);
        assert_eq!(quantize_scale(0.73), 0.75);
        assert_eq!(quantize_scale(f32::NAN), 1.0);
    }
    #[test]
    fn target_derivation() {
        let mut c = DynamicResolutionController::new(DynamicResolutionConfig {
            fps_cap: 50,
            ..Default::default()
        });
        assert_eq!(c.target_budget(), Duration::from_millis(20));
        c = DynamicResolutionController::new(DynamicResolutionConfig {
            fps_cap: 0,
            target_hz: 100.0,
            ..Default::default()
        });
        assert_eq!(c.target_budget(), Duration::from_millis(10));
    }
    #[test]
    fn hysteresis_and_cooldown() {
        let mut c = DynamicResolutionController::new(DynamicResolutionConfig {
            fallback_scale: 1.0,
            ..Default::default()
        });
        assert_eq!(c.submit(timing(1, 18)), 1.0); // inside 10% at 60 Hz
        assert_eq!(c.submit(timing(2, 30)), 0.9375);
        for tag in 3..33 {
            c.submit(timing(tag, 30));
        }
        assert_eq!(c.scale(), 0.9375); // 30-frame cooldown
        c.submit(timing(33, 30));
        assert_eq!(c.scale(), 0.875);
    }
    #[test]
    fn stale_out_of_order_and_missing_are_ignored() {
        let mut c = DynamicResolutionController::new(Default::default());
        c.submit(timing(10, 30));
        assert_eq!(c.submit(timing(9, 1)), 0.9375);
        assert_eq!(
            c.submit(GpuFrameTiming {
                frame_tag: 11,
                total: None
            }),
            1.0
        );
    }
    #[test]
    fn unsupported_falls_back_without_fake_data() {
        let mut c = DynamicResolutionController::new(DynamicResolutionConfig {
            fallback_scale: 0.67,
            gpu_timestamps_supported: false,
            ..Default::default()
        });
        assert_eq!(c.scale(), 0.6875);
        assert_eq!(c.submit(timing(1, 100)), 0.6875);
    }
    #[test]
    fn resize_and_minimize_are_safe() {
        let key = RenderTargetKey {
            extent: scaled_extent(800, 600, 0.75),
            scale: 0.75,
        };
        assert!(!should_recreate_target(Some(key), 800, 600, 0.751));
        assert!(should_recreate_target(Some(key), 1024, 600, 0.75));
        assert!(!should_recreate_target(Some(key), 0, 600, 0.5));
        assert_eq!(
            scaled_extent(0, 600, 0.5),
            RenderExtent {
                width: 0,
                height: 0
            }
        );
    }
    #[test]
    fn stable_convergence_hits_lower_bound() {
        let mut c = DynamicResolutionController::new(Default::default());
        let mut tag = 1;
        while c.scale() > MIN_SCALE {
            c.submit(timing(tag, 30));
            tag += 1;
            for _ in 0..ADJUSTMENT_COOLDOWN_FRAMES {
                c.submit(timing(tag, 30));
                tag += 1;
            }
        }
        assert_eq!(c.scale(), MIN_SCALE);
    }
}
