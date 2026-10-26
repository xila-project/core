use crate::model::MemorySample;
use alloc::{ffi::CString, format};
use core::num::NonZeroUsize;
use xila::graphics::{self, EventKind, OwnedWindow, lvgl};

pub enum Action {
    Close,
    ToggleWorkload,
    Interval,
}

pub struct View {
    graphics: &'static graphics::Manager,
    window: OwnedWindow,
    uptime: *mut lvgl::lv_obj_t,
    tasks: *mut lvgl::lv_obj_t,
    memory: *mut lvgl::lv_obj_t,
    capacity: *mut lvgl::lv_obj_t,
    status: *mut lvgl::lv_obj_t,
    toggle: *mut lvgl::lv_obj_t,
    toggle_text: *mut lvgl::lv_obj_t,
    interval: *mut lvgl::lv_obj_t,
    interval_text: *mut lvgl::lv_obj_t,
}

impl View {
    pub async fn new(graphics: &'static graphics::Manager) -> Result<Self, NonZeroUsize> {
        // Drop the lock before OwnedWindow on every construction error.
        let mut window;
        let _lock = graphics.lock().await;
        window = graphics.create_window().await.map_err(|_| crate::code(1))?;
        window.set_icon("Ac", graphics::Color::BLACK);
        let root = window.as_object_mutable();
        unsafe {
            lvgl::lv_obj_set_layout(root, lvgl::lv_layout_t_LV_LAYOUT_FLEX);
            lvgl::lv_obj_set_flex_flow(root, lvgl::lv_flex_flow_t_LV_FLEX_FLOW_COLUMN);
            lvgl::lv_obj_set_style_pad_all(root, 16, lvgl::LV_STATE_DEFAULT);
            label(root, xila::internationalization::translate!("app.title"))?;
            label(root, xila::internationalization::translate!("app.sources"))?;
            let uptime = label(root, "")?;
            let tasks = label(root, "")?;
            let memory = label(root, "")?;
            let capacity = label(root, "")?;
            label(
                root,
                xila::internationalization::translate!("metrics.unsupported"),
            )?;
            let (interval, interval_text) = button(root)?;
            let (toggle, toggle_text) = button(root)?;
            let status = label(root, "")?;
            Ok(Self {
                graphics,
                window,
                uptime,
                tasks,
                memory,
                capacity,
                status,
                toggle,
                toggle_text,
                interval,
                interval_text,
            })
        }
    }

    pub async fn action(&mut self) -> Option<Action> {
        let _lock = self.graphics.lock().await;
        while let Some(event) = self.window.pop_event() {
            if matches!(event.code, EventKind::Delete | EventKind::CloseRequested)
                && event.target == self.window.as_object_mutable()
            {
                return Some(Action::Close);
            }
            if event.code == EventKind::Clicked {
                if event.target == self.toggle {
                    return Some(Action::ToggleWorkload);
                }
                if event.target == self.interval {
                    return Some(Action::Interval);
                }
            }
        }
        None
    }

    pub async fn refresh(&mut self, uptime: u64, tasks: usize, memory: MemorySample) {
        let _lock = self.graphics.lock().await;
        unsafe {
            text(
                self.uptime,
                &format!(
                    xila::internationalization::translate!("metrics.uptime"),
                    uptime
                ),
            );
            text(
                self.tasks,
                &format!(
                    xila::internationalization::translate!("metrics.tasks"),
                    tasks
                ),
            );
            text(
                self.memory,
                &format!(
                    xila::internationalization::translate!("metrics.memory_used"),
                    memory.used
                ),
            );
            text(
                self.capacity,
                &match memory.capacity {
                    Some(bytes) => {
                        format!(
                            xila::internationalization::translate!("metrics.memory_capacity"),
                            bytes
                        )
                    }
                    None => xila::internationalization::translate!("metrics.capacity_unavailable")
                        .into(),
                },
            );
        }
    }

    pub async fn controls(&mut self, interval: u64, running: bool, failed: bool) {
        let _lock = self.graphics.lock().await;
        unsafe {
            text(
                self.interval_text,
                &format!(
                    xila::internationalization::translate!("actions.interval"),
                    interval
                ),
            );
            text(
                self.toggle_text,
                if running {
                    xila::internationalization::translate!("actions.stop")
                } else {
                    xila::internationalization::translate!("actions.start")
                },
            );
            text(
                self.status,
                if failed {
                    xila::internationalization::translate!("workload.failed")
                } else if running {
                    xila::internationalization::translate!("workload.running")
                } else {
                    xila::internationalization::translate!("workload.idle")
                },
            );
        }
    }

    pub async fn wait(&mut self, duration: core::time::Duration) {
        embassy_futures::select::select(
            xila::task::suspend(|context| self.window.register_waker(context.waker())),
            xila::task::sleep(duration),
        )
        .await;
    }
}

// All helpers are called under the graphics lock, with live widget pointers.
unsafe fn label(
    parent: *mut lvgl::lv_obj_t,
    value: &str,
) -> Result<*mut lvgl::lv_obj_t, NonZeroUsize> {
    unsafe {
        let label = lvgl::lv_label_create(parent);
        if label.is_null() {
            return Err(crate::code(1));
        }
        lvgl::lv_obj_set_width(label, lvgl::lv_pct(100));
        text(label, value);
        Ok(label)
    }
}
unsafe fn button(
    parent: *mut lvgl::lv_obj_t,
) -> Result<(*mut lvgl::lv_obj_t, *mut lvgl::lv_obj_t), NonZeroUsize> {
    unsafe {
        let button = lvgl::lv_button_create(parent);
        if button.is_null() {
            return Err(crate::code(1));
        }
        lvgl::lv_obj_add_flag(button, lvgl::lv_obj_flag_t_LV_OBJ_FLAG_EVENT_BUBBLE);
        lvgl::lv_obj_set_width(button, lvgl::lv_pct(100));
        let text = label(button, "")?;
        Ok((button, text))
    }
}
unsafe fn text(label: *mut lvgl::lv_obj_t, value: &str) {
    // Catalog text and numeric substitutions contain no interior NULs.
    if let Ok(value) = CString::new(value) {
        unsafe {
            lvgl::lv_label_set_text(label, value.as_ptr());
        }
    }
}
