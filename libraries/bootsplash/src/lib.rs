#![no_std]

extern crate alloc;

use core::ffi::c_void;

use graphics::{
    Color, Logo,
    lvgl::{
        _lv_obj_t, LV_PART_MAIN, LV_STATE_DEFAULT, lv_align_t_LV_ALIGN_CENTER,
        lv_anim_completed_cb_t, lv_anim_delete, lv_anim_exec_xcb_t, lv_anim_init,
        lv_anim_set_completed_cb, lv_anim_set_duration, lv_anim_set_exec_cb,
        lv_anim_set_repeat_count, lv_anim_set_values, lv_anim_set_var, lv_anim_start, lv_anim_t,
        lv_obj_get_child, lv_obj_get_size, lv_obj_get_style_text_color, lv_obj_get_width,
        lv_obj_set_align, lv_obj_set_style_opa, lv_obj_set_style_translate_x,
        lv_obj_set_style_translate_y,
    },
};

mod error;

pub use error::{Error, Result};

/// The symbol assembles itself, then a ripple runs clockwise around it.
///
/// Intro: the center square fades in, then each arm slides inward along its
/// diagonal into place, one after the other in clockwise order. The intro ends
/// on the exact brand symbol.
///
/// Loop: one arm at a time glides outward along its diagonal, briefly widening
/// its separation, and returns; the next arm (clockwise) starts halfway through.
/// The symbol is never rotated, recolored or closed up, and the arms are back
/// in place between pulses.
mod timeline {
    /// Fixed-point unit for progress values.
    pub const ONE: i32 = 1024;

    pub const SQUARE_FADE: i32 = 240;
    pub const ARM_DELAY: i32 = 150;
    pub const ARM_STAGGER: i32 = 100;
    pub const ARM_DURATION: i32 = 450;
    pub const INTRO: i32 = ARM_DELAY + 3 * ARM_STAGGER + ARM_DURATION;

    pub const PERIOD: i32 = 1600;
    pub const PULSE: i32 = PERIOD / 2;
    /// Opacity of an arm at the top of its pulse.
    pub const PULSE_OPACITY: i32 = 180;

    /// Clockwise rank of each arm in `Logo` child order (TL, BL, BR, TR).
    pub const CLOCKWISE_RANK: [i32; 4] = [0, 3, 2, 1];
    /// Outward diagonal of each arm in `Logo` child order.
    pub const DIRECTION: [(i32, i32); 4] = [(-1, -1), (-1, 1), (1, 1), (1, -1)];

    /// Outward offset in pixels and opacity of an arm.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct ArmState {
        pub offset: i32,
        pub opacity: u8,
    }

    pub const REST: ArmState = ArmState {
        offset: 0,
        opacity: 255,
    };

    fn progress(time: i32, start: i32, duration: i32) -> i32 {
        ((time - start) * ONE / duration).clamp(0, ONE)
    }

    fn ease_out_cubic(progress: i32) -> i32 {
        let remaining = ONE - progress;
        ONE - remaining * remaining / ONE * remaining / ONE
    }

    /// Smooth 0 → 1 → 0 bump with zero slope at both ends.
    fn bump(progress: i32) -> i32 {
        let parabola = 4 * progress * (ONE - progress) / ONE;
        parabola * parabola / ONE
    }

    /// Distance an arm travels during the intro.
    pub fn intro_distance(size: i32) -> i32 {
        size / 5
    }

    /// Distance an arm travels at the top of its pulse.
    pub fn pulse_distance(size: i32) -> i32 {
        (size / 20).max(1)
    }

    pub fn square_opacity(time: i32) -> u8 {
        (progress(time, 0, SQUARE_FADE) * 255 / ONE) as u8
    }

    pub fn intro_arm(index: usize, time: i32, size: i32) -> ArmState {
        let start = ARM_DELAY + CLOCKWISE_RANK[index] * ARM_STAGGER;
        let eased = ease_out_cubic(progress(time, start, ARM_DURATION));

        ArmState {
            offset: (ONE - eased) * intro_distance(size) / ONE,
            opacity: (eased * 255 / ONE) as u8,
        }
    }

    /// State of an arm `time` milliseconds into a ripple cycle. During the first
    /// cycle, arms whose turn has not come yet stay at rest.
    pub fn loop_arm(index: usize, time: i32, size: i32, first_cycle: bool) -> ArmState {
        let start = CLOCKWISE_RANK[index] * PERIOD / 4;
        if first_cycle && time < start {
            return REST;
        }
        let phase = (time - start).rem_euclid(PERIOD);
        if phase >= PULSE {
            return REST;
        }
        let amount = bump(phase * ONE / PULSE);

        ArmState {
            offset: amount * pulse_distance(size) / ONE,
            opacity: (255 - amount * (255 - PULSE_OPACITY) / ONE) as u8,
        }
    }
}

use timeline::ArmState;

unsafe fn apply_arm(logo: *mut _lv_obj_t, index: usize, state: ArmState) {
    let (x, y) = timeline::DIRECTION[index];

    unsafe {
        let arm = lv_obj_get_child(logo, index as i32);
        lv_obj_set_style_translate_x(arm, x * state.offset, LV_STATE_DEFAULT);
        lv_obj_set_style_translate_y(arm, y * state.offset, LV_STATE_DEFAULT);
        lv_obj_set_style_opa(arm, state.opacity, LV_STATE_DEFAULT);
    }
}

/// Show the intro `time` milliseconds after it started.
///
/// # Safety
/// `logo` must be a live [`Logo`] object and the caller must hold the LVGL lock.
unsafe fn show_intro(logo: *mut _lv_obj_t, time: i32) {
    unsafe {
        let size = lv_obj_get_width(logo);
        let center = lv_obj_get_child(logo, Logo::CENTER_INDEX as i32);
        lv_obj_set_style_opa(center, timeline::square_opacity(time), LV_STATE_DEFAULT);
        for index in 0..Logo::ARM_COUNT {
            apply_arm(logo, index, timeline::intro_arm(index, time, size));
        }
    }
}

/// Show the ripple `time` milliseconds into a cycle.
///
/// # Safety
/// `logo` must be a live [`Logo`] object and the caller must hold the LVGL lock.
unsafe fn show_loop(logo: *mut _lv_obj_t, time: i32, first_cycle: bool) {
    unsafe {
        let size = lv_obj_get_width(logo);
        for index in 0..Logo::ARM_COUNT {
            apply_arm(
                logo,
                index,
                timeline::loop_arm(index, time, size, first_cycle),
            );
        }
    }
}

unsafe extern "C" fn intro_callback(object: *mut c_void, value: i32) {
    unsafe { show_intro(object as *mut _lv_obj_t, value) }
}

unsafe extern "C" fn first_cycle_callback(object: *mut c_void, value: i32) {
    unsafe { show_loop(object as *mut _lv_obj_t, value, true) }
}

unsafe extern "C" fn cycle_callback(object: *mut c_void, value: i32) {
    unsafe { show_loop(object as *mut _lv_obj_t, value, false) }
}

unsafe extern "C" fn intro_completed(animation: *mut lv_anim_t) {
    unsafe {
        start(
            (*animation).var,
            timeline::PERIOD,
            Some(first_cycle_callback),
            0,
            Some(first_cycle_completed),
        );
    }
}

unsafe extern "C" fn first_cycle_completed(animation: *mut lv_anim_t) {
    unsafe {
        start(
            (*animation).var,
            timeline::PERIOD,
            Some(cycle_callback),
            u32::MAX,
            None,
        );
    }
}

/// Start an animation driving `callback` with the elapsed time in milliseconds.
///
/// LVGL copies the descriptor, so it can live on the stack.
unsafe fn start(
    logo: *mut c_void,
    duration: i32,
    callback: lv_anim_exec_xcb_t,
    repeat_count: u32,
    completed: lv_anim_completed_cb_t,
) {
    unsafe {
        let mut animation = lv_anim_t::default();
        lv_anim_init(&mut animation);
        lv_anim_set_var(&mut animation, logo);
        lv_anim_set_values(&mut animation, 0, duration);
        lv_anim_set_duration(&mut animation, duration as u32);
        lv_anim_set_repeat_count(&mut animation, repeat_count);
        lv_anim_set_exec_cb(&mut animation, callback);
        lv_anim_set_completed_cb(&mut animation, completed);
        lv_anim_start(&animation);
    }
}

pub struct Bootsplash {
    logo: Logo,
}

impl Bootsplash {
    pub async fn new(graphics_manager: &'static graphics::Manager) -> Result<Self> {
        let _lock = graphics_manager.lock().await;

        unsafe {
            let current_screen = graphics_manager.get_current_screen()?;

            let screen_size = lv_obj_get_size(current_screen);
            let factor = Logo::get_factor(screen_size.scale(0.3)).max(1);

            let foreground =
                Color::from_lvgl_color(lv_obj_get_style_text_color(current_screen, LV_PART_MAIN));
            let logo = Logo::new(current_screen, factor, foreground)?;

            let object = logo.get_inner_object();
            lv_obj_set_align(object, lv_align_t_LV_ALIGN_CENTER);
            show_intro(object, 0);

            start(
                object as *mut c_void,
                timeline::INTRO,
                Some(intro_callback),
                0,
                Some(intro_completed),
            );

            Ok(Self { logo })
        }
    }

    pub async fn stop(self, graphics_manager: &'static graphics::Manager) -> Result<()> {
        let _lock = graphics_manager.lock().await;

        unsafe {
            let object = self.logo.get_inner_object() as *mut c_void;
            lv_anim_delete(object, Some(intro_callback));
            lv_anim_delete(object, Some(first_cycle_callback));
            lv_anim_delete(object, Some(cycle_callback));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::timeline::*;

    const SIZE: i32 = 160;

    #[test]
    fn intro_starts_hidden_and_ends_on_the_exact_symbol() {
        assert_eq!(square_opacity(0), 0);
        assert_eq!(square_opacity(INTRO), 255);
        for index in 0..4 {
            let start = intro_arm(index, 0, SIZE);
            assert_eq!(start.opacity, 0);
            assert_eq!(start.offset, intro_distance(SIZE));
            assert_eq!(intro_arm(index, INTRO, SIZE), REST);
        }
    }

    #[test]
    fn intro_assembles_arms_clockwise() {
        // Top-left, then top-right, bottom-right and bottom-left.
        let time = ARM_DELAY + ARM_STAGGER + ARM_STAGGER / 2;
        let progress = |index| intro_arm(index, time, SIZE).opacity;
        assert!(progress(0) > progress(3));
        assert!(progress(3) > progress(2));
        assert_eq!(progress(2), 0);
        assert_eq!(progress(1), 0);
    }

    #[test]
    fn ripple_moves_outward_only_and_joins_the_intro_smoothly() {
        for first_cycle in [true, false] {
            for index in 0..4 {
                for time in (0..PERIOD).step_by(10) {
                    let state = loop_arm(index, time, SIZE, first_cycle);
                    assert!((0..=pulse_distance(SIZE)).contains(&state.offset));
                    assert!(state.opacity as i32 >= PULSE_OPACITY);
                }
                // Both cycles start from, and every cycle wraps to, the rest pose.
                assert_eq!(loop_arm(index, 0, SIZE, true), REST);
            }
        }
        assert_eq!(loop_arm(0, 0, SIZE, false), REST);
        // The pulse peaks halfway and the next arm clockwise follows it.
        assert_eq!(
            loop_arm(0, PULSE / 2, SIZE, false).offset,
            pulse_distance(SIZE)
        );
        assert_eq!(
            loop_arm(3, PERIOD / 4 + PULSE / 2, SIZE, false).offset,
            pulse_distance(SIZE)
        );
        // Arms not yet reached in the first cycle stay put.
        assert_eq!(loop_arm(1, PERIOD / 8, SIZE, true), REST);
        assert_ne!(loop_arm(1, PERIOD / 8, SIZE, false), REST);
    }
}
