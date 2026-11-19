//! Render the logo into a small headless LVGL snapshot.
#![cfg(target_os = "linux")]

extern crate abi_definitions;

testing::instantiate_global_allocator!();

use graphics::{Color, Logo, LogoStyle, lvgl};

#[test]
fn logo_snapshot_preserves_geometry_and_blends_opacity() {
    unsafe {
        lvgl::lv_init();
        for (factor, square_half) in [(1, 3), (2, 6), (4, 12), (5, 15)] {
            let size = 32 * i32::from(factor);
            let mid = size / 2;
            let display = lvgl::lv_display_create(size, size);
            let screen = lvgl::lv_display_get_screen_active(display);
            lvgl::lv_obj_remove_style_all(screen);
            lvgl::lv_obj_set_style_bg_color(screen, Color::BLACK.into_lvgl_color(), 0);
            lvgl::lv_obj_set_style_bg_opa(screen, 255, 0);
            let parent = lvgl::lv_obj_create(screen);
            lvgl::lv_obj_remove_style_all(parent);
            lvgl::lv_obj_set_size(parent, size, size);
            let format = lvgl::lv_color_format_t_LV_COLOR_FORMAT_XRGB8888;
            let buffer = lvgl::lv_snapshot_create_draw_buf(screen, format);
            assert!(!buffer.is_null());
            let draw = || {
                assert_eq!(
                    lvgl::lv_snapshot_take_to_draw_buf(screen, format, buffer),
                    lvgl::lv_result_t_LV_RESULT_OK,
                );
            };
            let pixel = |x: i32, y: i32| {
                let offset = y as usize * (*buffer).header.stride() as usize + x as usize * 4;
                let p = (*buffer).data.add(offset);
                [*p, *p.add(1), *p.add(2)]
            };
            for style in [LogoStyle::Filled, LogoStyle::Outline] {
                let logo = Logo::new_with_style(parent, factor, Color::WHITE, style).unwrap();
                draw();
                let expect = |x, y, value| {
                    assert_eq!(
                        pixel(x, y),
                        [value; 3],
                        "factor {factor}, {style:?} at ({x}, {y})"
                    );
                };

                // The horizontal and vertical gaps between the arms.
                for (x, y) in [
                    (mid, mid / 2),
                    (mid, size - 1 - mid / 2),
                    (mid / 2, mid),
                    (size - 1 - mid / 2, mid),
                ] {
                    expect(x, y, 0);
                }

                let arm_x = size - 1 - size / 8;
                let arm_y = if style == LogoStyle::Filled {
                    size / 16
                } else {
                    0
                };
                for mirror_x in [false, true] {
                    for mirror_y in [false, true] {
                        let x = |x| if mirror_x { size - 1 - x } else { x };
                        let y = |y| if mirror_y { size - 1 - y } else { y };
                        // An opaque sample in the arm's top run, and a clear
                        // sample just outside the square in the arm's notch.
                        expect(x(arm_x), y(arm_y), 255);
                        expect(x(mid + square_half), y(mid - square_half), 0);
                    }
                }
                let square_y = if style == LogoStyle::Filled {
                    mid
                } else {
                    mid - square_half
                };
                expect(mid, square_y, 255);

                if factor == 4 {
                    for (logo_opa, parent_opa, expected) in
                        [(128, 255, 128_u8), (255, 128, 128), (128, 128, 64)]
                    {
                        lvgl::lv_obj_set_style_opa(logo.get_inner_object(), logo_opa, 0);
                        lvgl::lv_obj_set_style_opa(parent, parent_opa, 0);
                        draw();
                        for (x, y) in [(arm_x, arm_y), (mid, square_y)] {
                            let rgb = pixel(x, y);
                            // Allow for LVGL's fixed-point opacity and RGB rounding.
                            assert!(
                                rgb.into_iter()
                                    .all(|channel| channel.abs_diff(expected) <= 4),
                                "{style:?} at ({x}, {y}): expected about {expected}, got {rgb:?}",
                            );
                        }
                    }
                    lvgl::lv_obj_set_style_opa(parent, 255, 0);
                }
            }
            lvgl::lv_draw_buf_destroy(buffer);
            lvgl::lv_display_delete(display);
        }
        lvgl::lv_deinit();
    }
}
