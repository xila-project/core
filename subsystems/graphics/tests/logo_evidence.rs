//! Opt-in headless capture and measurement for the logo renderer.
#![cfg(all(target_os = "linux", lvgl_system_heap))]

extern crate abi_definitions;

use std::{cell::Cell, fmt::Write as _, fs, path::Path, ptr::NonNull, sync::Mutex, time::Instant};

use graphics::{Color, Logo, LogoStyle, lvgl};
use memory::{CapabilityFlags, Layout, ManagerTrait};
use testing::{MemoryManager, ThreadAllocations};

// Both opt-in tests use the process-wide LVGL instance.
static LVGL: Mutex<()> = Mutex::new(());

struct MeasuringAllocator(MemoryManager);

thread_local! {
    static PEAK: Cell<Option<(ThreadAllocations, isize)>> = const { Cell::new(None) };
}

impl ManagerTrait for MeasuringAllocator {
    unsafe fn allocate(&self, flags: CapabilityFlags, layout: Layout) -> Option<NonNull<u8>> {
        let allocation = unsafe { self.0.allocate(flags, layout) };
        PEAK.with(|peak| {
            if let Some((before, high)) = peak.get() {
                let current = MemoryManager::thread_allocations()
                    .since(before)
                    .net_bytes();
                peak.set(Some((before, high.max(current))));
            }
        });
        allocation
    }

    unsafe fn deallocate(&self, pointer: NonNull<u8>, layout: Layout) {
        unsafe { self.0.deallocate(pointer, layout) };
    }

    fn get_used(&self) -> usize {
        self.0.get_used()
    }

    fn get_free(&self) -> usize {
        self.0.get_free()
    }
}

static ALLOCATOR: MeasuringAllocator = MeasuringAllocator(MemoryManager::new());
memory::instantiate_global_allocator!(&ALLOCATOR);

fn measure_heap(draw: impl FnOnce()) -> (isize, isize) {
    let before = MemoryManager::thread_allocations();
    PEAK.set(Some((before, 0)));
    draw();
    let (_, peak) = PEAK.take().unwrap();
    let net = MemoryManager::thread_allocations()
        .since(before)
        .net_bytes();
    (peak, net)
}

fn median_ns(draw: impl Fn()) -> u128 {
    let mut samples = [0; 9];
    for sample in &mut samples {
        let start = Instant::now();
        for _ in 0..128 {
            draw();
        }
        *sample = start.elapsed().as_nanos() / 128;
    }
    samples.sort_unstable();
    samples[samples.len() / 2]
}

#[test]
#[ignore = "writes comparison inputs and measures host redraws"]
fn capture_logo_evidence() {
    let _lock = LVGL.lock().unwrap();
    let output = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/logo-capture");
    fs::create_dir_all(&output).unwrap();
    let mut report = String::from(
        "size,style,background,format,object_bytes,cold_peak,cold_retained,warm_peak,warm_retained,background_peak,snapshot_ns,background_ns\n",
    );
    for factor in [1, 2, 4, 5] {
        for (style_name, style) in [
            ("filled", LogoStyle::Filled),
            ("outline", LogoStyle::Outline),
        ] {
            for (background, bg, fg) in [
                ("light", Color::new(244, 244, 245), Color::new(24, 24, 27)),
                ("dark", Color::new(24, 24, 27), Color::new(244, 244, 245)),
            ] {
                for (format, cf) in [
                    ("rgb565", lvgl::lv_color_format_t_LV_COLOR_FORMAT_RGB565),
                    ("xrgb8888", lvgl::lv_color_format_t_LV_COLOR_FORMAT_XRGB8888),
                ] {
                    let size = i32::from(factor) * 32;
                    unsafe {
                        lvgl::lv_init();
                        let display = lvgl::lv_display_create(size, size);
                        lvgl::lv_display_set_color_format(display, cf);
                        let screen = lvgl::lv_display_get_screen_active(display);
                        lvgl::lv_obj_remove_style_all(screen);
                        lvgl::lv_obj_set_style_bg_opa(screen, 255, 0);
                        lvgl::lv_obj_set_style_bg_color(screen, bg.into_lvgl_color(), 0);
                        let buffer = lvgl::lv_snapshot_create_draw_buf(screen, cf);
                        assert!(!buffer.is_null());
                        let draw = || {
                            assert_eq!(
                                lvgl::lv_snapshot_take_to_draw_buf(screen, cf, buffer),
                                lvgl::lv_result_t_LV_RESULT_OK,
                            );
                        };
                        draw();
                        let (background_peak, _) = measure_heap(draw);
                        let background_ns = median_ns(draw);
                        let before = MemoryManager::thread_allocations();
                        let logo = Logo::new_with_style(screen, factor, fg, style).unwrap();
                        lvgl::lv_obj_align(
                            logo.get_inner_object(),
                            lvgl::lv_align_t_LV_ALIGN_CENTER,
                            0,
                            0,
                        );
                        lvgl::lv_obj_update_layout(screen);
                        let object_bytes = MemoryManager::thread_allocations()
                            .since(before)
                            .net_bytes();
                        let (cold_peak, cold_retained) = measure_heap(draw);
                        let (warm_peak, warm_retained) = measure_heap(draw);
                        let snapshot_ns = median_ns(draw);
                        let bytes = std::slice::from_raw_parts(
                            (*buffer).data,
                            (*buffer).header.stride() as usize * size as usize,
                        );
                        fs::write(
                            output.join(format!("{size}-{style_name}-{background}-{format}.raw")),
                            bytes,
                        )
                        .unwrap();
                        writeln!(
                            report,
                            "{size},{style_name},{background},{format},{object_bytes},{cold_peak},{cold_retained},{warm_peak},{warm_retained},{background_peak},{snapshot_ns},{background_ns}",
                        )
                        .unwrap();
                        drop(logo);
                        lvgl::lv_draw_buf_destroy(buffer);
                        lvgl::lv_display_delete(display);
                        lvgl::lv_deinit();
                    }
                }
            }
        }
    }
    fs::write(output.join("measurements.csv"), report).unwrap();
}

/// Count allocations on the render thread with LVGL in partial render mode.
#[test]
#[ignore = "measures host redraw allocations"]
fn count_logo_redraw_allocations() {
    let _lock = LVGL.lock().unwrap();
    const SIZE: i32 = 128;
    unsafe extern "C" fn flush(
        display: *mut lvgl::lv_display_t,
        _: *const lvgl::lv_area_t,
        _: *mut u8,
    ) {
        unsafe { lvgl::lv_display_flush_ready(display) };
    }

    unsafe {
        lvgl::lv_init();
        let display = lvgl::lv_display_create(SIZE, SIZE);
        lvgl::lv_display_set_color_format(
            display,
            lvgl::lv_color_format_t_LV_COLOR_FORMAT_XRGB8888,
        );
        let mut pixels = vec![0_u32; (SIZE * SIZE) as usize];
        lvgl::lv_display_set_buffers(
            display,
            pixels.as_mut_ptr().cast(),
            std::ptr::null_mut(),
            (pixels.len() * size_of::<u32>()) as u32,
            lvgl::lv_display_render_mode_t_LV_DISPLAY_RENDER_MODE_PARTIAL,
        );
        lvgl::lv_display_set_flush_cb(display, Some(flush));
        let screen = lvgl::lv_display_get_screen_active(display);
        lvgl::lv_obj_remove_style_all(screen);
        lvgl::lv_obj_set_style_bg_opa(screen, 255, 0);
        lvgl::lv_refr_now(display);

        let count = |y1, y2| {
            let area = lvgl::lv_area_t {
                x1: 0,
                x2: SIZE - 1,
                y1,
                y2,
            };
            let redraw = || {
                assert_eq!(
                    lvgl::lv_obj_invalidate_area(screen, &area),
                    lvgl::lv_result_t_LV_RESULT_OK,
                );
                lvgl::lv_refr_now(display);
            };
            redraw();
            let before = MemoryManager::thread_allocations();
            redraw();
            MemoryManager::thread_allocations()
                .since(before)
                .allocations()
        };
        let full_background = count(0, SIZE - 1);
        let strip_background = count(SIZE / 2 - 1, SIZE / 2);
        for style in [LogoStyle::Filled, LogoStyle::Outline] {
            let logo = Logo::new_with_style(screen, 4, Color::WHITE, style).unwrap();
            lvgl::lv_refr_now(display);
            let full = count(0, SIZE - 1);
            let strip = count(SIZE / 2 - 1, SIZE / 2);
            println!(
                "{style:?}: full {} ({full} total), center strip {} ({strip} total)",
                full - full_background,
                strip - strip_background,
            );
            drop(logo);
        }
        lvgl::lv_display_delete(display);
        lvgl::lv_deinit();
    }
}
