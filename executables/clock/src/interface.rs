use clock::{Alarm, AlarmAlert, Settings, Stopwatch, Timer, storage::Store};
use embedded_i18n::translate;
use std::{
    ffi::{CStr, CString},
    ptr::null_mut,
    thread::sleep,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use wasm::*;

#[derive(Clone, Copy)]
enum Action {
    Format,
    AlarmHour(i8),
    AlarmMinute(i8),
    AlarmSave,
    AlarmDisable,
    AlarmDelete,
    StopwatchToggle,
    Lap,
    StopwatchReset,
    TimerAdjust(i32),
    TimerStart,
    TimerToggle,
    TimerRestart,
    TimerCancel,
    Snooze,
    Dismiss,
}

struct Interface {
    window: *mut Object,
    tabs: *mut Object,
    controls: Vec<(*mut Object, Action)>,
    labels: [*mut Object; 6],
    rendered: [String; 6],
    weekdays: [*mut Object; 7],
    settings: Settings,
    store: Option<Store>,
    persistence_error: bool,
    watch: Stopwatch,
    timer: Timer,
    timer_seconds: u32,
    alarm_hour: u8,
    alarm_minute: u8,
    alarm_alert: AlarmAlert,
}

pub fn run() -> wasm::Result<()> {
    // The native epoch is supplied through WASI's monotonic clock.
    let epoch = Instant::now();
    let mut ui = unsafe { Interface::new()? };
    let mut last_refresh = None;
    loop {
        let now = epoch.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
        let wall = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .map(|value| value.as_secs());
        let mut dirty = ui.tick(now, wall);
        // Bound work per turn even if an input device floods the window queue.
        for _ in 0..32 {
            let mut code = 0u32;
            let mut target: *mut Object = null_mut();
            unsafe {
                window_pop_event(
                    ui.window,
                    &mut code,
                    (&mut target as *mut *mut Object).cast(),
                )?;
            }
            if code == EVENT_CLOSE_REQUESTED {
                unsafe {
                    object_delete(u16::try_from(ui.window as usize).map_err(|_| -1)?)?;
                }
                return Ok(());
            }
            if code == EventCode::All as u32 {
                break;
            }
            if code == EventCode::Clicked as u32
                && let Some((_, action)) = ui
                    .controls
                    .iter()
                    .find(|(object, _)| *object == target)
                    .copied()
            {
                unsafe {
                    ui.act(action, now, wall)?;
                }
                dirty = true;
            }
            dirty |= code == EventCode::ValueChanged as u32;
        }
        let visible_stopwatch =
            unsafe { tabview_get_tab_active(ui.tabs)? } == 2 && ui.watch.is_running();
        let interval = if visible_stopwatch { 100 } else { 1000 };
        if dirty || last_refresh.is_none_or(|last| now.saturating_sub(last) >= interval) {
            unsafe {
                ui.render(now, wall)?;
            }
            last_refresh = Some(now);
        }
        // There is no blocking guest window-event API yet. Cooperatively wait
        // between bounded event checks; elapsed durations do not count polls.
        sleep(Duration::from_millis(if visible_stopwatch {
            100
        } else {
            250
        }));
    }
}

impl Interface {
    unsafe fn new() -> wasm::Result<Self> {
        unsafe {
            let (store, settings, persistence_error) =
                match std::env::var("XILA_APPLICATION_CONFIGURATION") {
                    Ok(directory) => match Store::load(&directory) {
                        Ok((store, settings)) => (Some(store), settings, false),
                        Err(error) => {
                            eprintln!("Clock settings unavailable: {error}");
                            (None, Settings::default(), true)
                        }
                    },
                    Err(error) => {
                        eprintln!("Clock settings directory unavailable: {error}");
                        (None, Settings::default(), true)
                    }
                };
            let window = window_create()?;
            // Ensure a partially constructed window is released on FFI failure.
            match Self::build(window, store, settings, persistence_error) {
                Ok(ui) => Ok(ui),
                Err(error) => {
                    if let Ok(handle) = u16::try_from(window as usize) {
                        let _ = object_delete(handle);
                    }
                    Err(error)
                }
            }
        }
    }

    unsafe fn build(
        window: *mut Object,
        store: Option<Store>,
        settings: Settings,
        persistence_error: bool,
    ) -> wasm::Result<Self> {
        unsafe {
            object_set_flex_flow(window, FlexFlow::Column)?;
            window_set_icon(
                window,
                c"Cl".as_ptr(),
                Color {
                    red: 59,
                    green: 130,
                    blue: 246,
                },
            )?;
            let mut ui = Self {
                window,
                tabs: null_mut(),
                controls: Vec::with_capacity(32),
                labels: [null_mut(); 6],
                rendered: core::array::from_fn(|_| String::new()),
                weekdays: [null_mut(); 7],
                settings,
                store,
                persistence_error,
                watch: Stopwatch::default(),
                timer: Timer::default(),
                timer_seconds: 300,
                alarm_hour: settings.alarm.map_or(7, |a| a.hour),
                alarm_minute: settings.alarm.map_or(0, |a| a.minute),
                alarm_alert: AlarmAlert::default(),
            };
            ui.labels[0] = ui.label(window, translate!("alerts.instructions"))?;
            let alert_row = ui.row(window)?;
            ui.button(
                alert_row,
                translate!(c"alerts.snooze_five_minutes"),
                Action::Snooze,
            )?;
            ui.button(alert_row, translate!(c"alerts.dismiss"), Action::Dismiss)?;

            let tabs = tabview_create(window)?;
            object_set_width(tabs, percentage(100)?)?;
            object_set_flex_grow(tabs, 1)?;
            ui.tabs = tabs;
            let clock_tab = ui.tab(translate!(c"tabs.clock"))?;
            ui.labels[1] = ui.label(clock_tab, "")?;
            ui.label(clock_tab, translate!("clock.instructions.utc_only"))?;
            ui.button(
                clock_tab,
                translate!(c"clock.controls.toggle_hour_format"),
                Action::Format,
            )?;

            let alarm_tab = ui.tab(translate!(c"tabs.alarm"))?;
            ui.labels[2] = ui.label(alarm_tab, "")?;
            let alarm_row = ui.row(alarm_tab)?;
            ui.button(
                alarm_row,
                translate!(c"alarm.controls.decrement_hour"),
                Action::AlarmHour(-1),
            )?;
            ui.button(
                alarm_row,
                translate!(c"alarm.controls.increment_hour"),
                Action::AlarmHour(1),
            )?;
            ui.button(
                alarm_row,
                translate!(c"alarm.controls.decrement_minute"),
                Action::AlarmMinute(-1),
            )?;
            ui.button(
                alarm_row,
                translate!(c"alarm.controls.increment_minute"),
                Action::AlarmMinute(1),
            )?;
            ui.label(alarm_tab, translate!("alarm.instructions.repeat_days_utc"))?;
            let days = [
                translate!(c"alarm.weekdays.monday"),
                translate!(c"alarm.weekdays.tuesday"),
                translate!(c"alarm.weekdays.wednesday"),
                translate!(c"alarm.weekdays.thursday"),
                translate!(c"alarm.weekdays.friday"),
                translate!(c"alarm.weekdays.saturday"),
                translate!(c"alarm.weekdays.sunday"),
            ];
            let day_row = ui.row(alarm_tab)?;
            for (index, day) in days.iter().enumerate() {
                let checkbox = checkbox_create(day_row)?;
                checkbox_set_text(checkbox, day.as_ptr())?;
                if settings
                    .alarm
                    .is_some_and(|alarm| alarm.weekdays & (1 << index) != 0)
                {
                    object_add_state(checkbox, STATE_CHECKED)?;
                }
                ui.weekdays[index] = checkbox;
            }
            let alarm_actions = ui.row(alarm_tab)?;
            ui.button(
                alarm_actions,
                translate!(c"alarm.controls.save_and_enable"),
                Action::AlarmSave,
            )?;
            ui.button(
                alarm_actions,
                translate!(c"alarm.controls.disable"),
                Action::AlarmDisable,
            )?;
            ui.button(
                alarm_actions,
                translate!(c"alarm.controls.delete"),
                Action::AlarmDelete,
            )?;

            let watch_tab = ui.tab(translate!(c"tabs.stopwatch"))?;
            ui.labels[3] = ui.label(watch_tab, "")?;
            let watch_actions = ui.row(watch_tab)?;
            ui.button(
                watch_actions,
                translate!(c"stopwatch.controls.toggle_pause"),
                Action::StopwatchToggle,
            )?;
            ui.button(
                watch_actions,
                translate!(c"stopwatch.controls.record_lap"),
                Action::Lap,
            )?;
            ui.button(
                watch_actions,
                translate!(c"stopwatch.controls.reset"),
                Action::StopwatchReset,
            )?;
            ui.labels[4] = ui.label(watch_tab, "")?;

            let timer_tab = ui.tab(translate!(c"tabs.timer"))?;
            ui.labels[5] = ui.label(timer_tab, "")?;
            let duration_row = ui.row(timer_tab)?;
            ui.button(
                duration_row,
                translate!(c"timer.controls.decrement_minute"),
                Action::TimerAdjust(-60),
            )?;
            ui.button(
                duration_row,
                translate!(c"timer.controls.increment_minute"),
                Action::TimerAdjust(60),
            )?;
            ui.button(
                duration_row,
                translate!(c"timer.controls.decrement_second"),
                Action::TimerAdjust(-1),
            )?;
            ui.button(
                duration_row,
                translate!(c"timer.controls.increment_second"),
                Action::TimerAdjust(1),
            )?;
            let timer_actions = ui.row(timer_tab)?;
            ui.button(
                timer_actions,
                translate!(c"timer.controls.start"),
                Action::TimerStart,
            )?;
            ui.button(
                timer_actions,
                translate!(c"timer.controls.toggle_pause"),
                Action::TimerToggle,
            )?;
            ui.button(
                timer_actions,
                translate!(c"timer.controls.restart"),
                Action::TimerRestart,
            )?;
            ui.button(
                timer_actions,
                translate!(c"timer.controls.cancel"),
                Action::TimerCancel,
            )?;
            Ok(ui)
        }
    }

    unsafe fn label(&self, parent: *mut Object, text: &str) -> wasm::Result<*mut Object> {
        unsafe {
            let label = label_create(parent)?;
            object_set_width(label, percentage(100)?)?;
            Self::text(label, text)?;
            Ok(label)
        }
    }
    unsafe fn text(label: *mut Object, text: &str) -> wasm::Result<()> {
        let text = CString::new(text).map_err(|_| -1)?;
        unsafe { label_set_text(label, text.as_ptr()) }
    }
    unsafe fn row(&self, parent: *mut Object) -> wasm::Result<*mut Object> {
        unsafe {
            let row = object_create(parent)?;
            object_set_width(row, percentage(100)?)?;
            object_set_height(row, size_content())?;
            object_set_flex_flow(row, FlexFlow::RowWrap)?;
            Ok(row)
        }
    }
    unsafe fn tab(&self, name: &CStr) -> wasm::Result<*mut Object> {
        unsafe {
            let tab = tabview_add_tab(self.tabs, name.as_ptr())?;
            object_set_flex_flow(tab, FlexFlow::Column)?;
            Ok(tab)
        }
    }
    unsafe fn button(
        &mut self,
        parent: *mut Object,
        text: &CStr,
        action: Action,
    ) -> wasm::Result<()> {
        unsafe {
            let button = button_create(parent)?;
            let label = label_create(button)?;
            label_set_text(label, text.as_ptr())?;
            // Window events identify the control rather than its child label.
            object_add_flag(button, ObjectFlag::EventBubble)?;
            self.controls.push((button, action));
            Ok(())
        }
    }
    fn persist(&mut self) {
        if let Some(store) = &mut self.store {
            self.persistence_error = match store.save(&self.settings) {
                Ok(()) => false,
                Err(error) => {
                    eprintln!("Clock settings could not be saved: {error}");
                    true
                }
            };
        }
    }
    fn tick(&mut self, now: u64, wall: Option<u64>) -> bool {
        let mut changed = self.timer.tick(now);
        if let (Some(alarm), Some(wall)) = (self.settings.alarm.as_mut(), wall)
            && alarm.tick(wall)
        {
            self.alarm_alert.ring();
            self.persist();
            changed = true;
        }
        self.alarm_alert.tick(now) || changed
    }
    unsafe fn act(&mut self, action: Action, now: u64, wall: Option<u64>) -> wasm::Result<()> {
        unsafe {
            match action {
                Action::Format => {
                    self.settings.twelve_hour = !self.settings.twelve_hour;
                    self.persist();
                }
                Action::AlarmHour(delta) => {
                    self.alarm_hour =
                        (i16::from(self.alarm_hour) + i16::from(delta)).rem_euclid(24) as u8
                }
                Action::AlarmMinute(delta) => {
                    self.alarm_minute =
                        (i16::from(self.alarm_minute) + i16::from(delta)).rem_euclid(60) as u8
                }
                Action::AlarmSave => {
                    if let Some(wall) = wall {
                        let mut days = 0;
                        for (index, day) in self.weekdays.iter().enumerate() {
                            if object_get_state(*day)? & STATE_CHECKED != 0 {
                                days |= 1 << index;
                            }
                        }
                        let mut alarm =
                            Alarm::new(self.alarm_hour, self.alarm_minute, days).unwrap();
                        alarm.arm(wall);
                        self.settings.alarm = Some(alarm);
                        self.alarm_alert.dismiss();
                        self.persist();
                    }
                }
                Action::AlarmDisable | Action::AlarmDelete => {
                    if let Some(alarm) = self.settings.alarm.as_mut() {
                        alarm.disable();
                    }
                    if matches!(action, Action::AlarmDelete) {
                        self.settings.alarm = None;
                    }
                    self.alarm_alert.dismiss();
                    self.persist();
                }
                Action::StopwatchToggle => {
                    if self.watch.is_running() {
                        self.watch.pause(now);
                    } else {
                        self.watch.start(now);
                    }
                }
                Action::Lap => {
                    self.watch.lap(now);
                }
                Action::StopwatchReset => self.watch.reset(),
                Action::TimerAdjust(delta) => {
                    self.timer_seconds =
                        (i64::from(self.timer_seconds) + i64::from(delta)).clamp(1, 86400) as u32
                }
                Action::TimerStart => {
                    self.timer.start(u64::from(self.timer_seconds) * 1000, now);
                }
                Action::TimerToggle => {
                    if self.timer.is_running() {
                        self.timer.pause(now);
                    } else {
                        self.timer.resume(now);
                    }
                }
                Action::TimerRestart => self.timer.restart(now),
                Action::TimerCancel => self.timer.cancel(),
                Action::Snooze => {
                    self.alarm_alert.snooze(now);
                }
                Action::Dismiss => {
                    self.alarm_alert.dismiss();
                    self.timer.acknowledge();
                }
            }
            Ok(())
        }
    }
    unsafe fn render(&mut self, now: u64, wall: Option<u64>) -> wasm::Result<()> {
        let active = unsafe { tabview_get_tab_active(self.tabs)? };
        let mut alert = String::new();
        if self.alarm_alert.is_ringing() {
            alert.push_str(translate!("alerts.alarm_ringing"));
        }
        if self.timer.finished() {
            alert.push_str(translate!("alerts.timer_finished"));
        }
        if self.alarm_alert.is_snoozed() {
            alert.push_str(translate!("alerts.alarm_snoozed"));
        }
        if self.persistence_error {
            alert.push_str(translate!("errors.settings_save_failed"));
        }
        if alert.is_empty() {
            alert.push_str(translate!("alerts.instructions"));
        }
        let clock = if active == 0 {
            wall.and_then(|wall| i64::try_from(wall).ok()).map_or_else(
                || translate!("clock.errors.host_unavailable").to_string(),
                |wall| {
                    let mut text = String::new();
                    let pattern = if self.settings.twelve_hour {
                        "%a %d %b %Y\n%I:%M:%S %p"
                    } else {
                        "%a %d %b %Y\n%H:%M:%S"
                    };
                    let _ = embedded_i18n::write_unix_timestamp(&mut text, wall, pattern);
                    text
                },
            )
        } else {
            String::new()
        };
        let mut alarm = String::new();
        if active == 1 {
            alarm = format!("{:02}:{:02} UTC\n", self.alarm_hour, self.alarm_minute);
            if let Some(saved) = self.settings.alarm {
                if saved.enabled() {
                    alarm.push_str(translate!("alarm.status.enabled"));
                    alarm.push_str(&format!("{:02}:{:02} UTC\n", saved.hour, saved.minute));
                    if let Some(next) = saved.next().and_then(|value| i64::try_from(value).ok()) {
                        let _ =
                            embedded_i18n::write_unix_timestamp(&mut alarm, next, "%a %d %b %H:%M");
                    }
                } else {
                    alarm.push_str(translate!("alarm.status.disabled"));
                }
            } else {
                alarm.push_str(translate!("alarm.status.no_saved_alarm"));
            }
        }
        let watch = if active == 2 {
            format_duration(self.watch.elapsed(now), true)
        } else {
            String::new()
        };
        let mut laps = String::new();
        if active == 2 {
            use core::fmt::Write;
            let count = self.watch.laps().len();
            if count != 0 {
                // Compiled locale forms; selection does not allocate.
                let _ = embedded_i18n::translate_plural_write!(
                    &mut laps,
                    "stopwatch.laps.recorded",
                    count
                );
                laps.push('\n');
            }
            for (index, lap) in self.watch.laps().iter().enumerate() {
                let _ = writeln!(laps, "{}: {}", index + 1, format_duration(*lap, true));
            }
        }
        let timer = if active == 3 {
            format!(
                "{}\n{}: {}",
                format_duration(self.timer.remaining(now).div_ceil(1000) * 1000, false),
                translate!("timer.labels.duration"),
                format_duration(u64::from(self.timer_seconds) * 1000, false)
            )
        } else {
            String::new()
        };
        for (index, text) in [alert, clock, alarm, watch, laps, timer]
            .into_iter()
            .enumerate()
        {
            if !matches!(
                (active, index),
                (_, 0) | (0, 1) | (1, 2) | (2, 3 | 4) | (3, 5)
            ) {
                continue;
            }
            if self.rendered[index] != text {
                unsafe {
                    Self::text(self.labels[index], &text)?;
                }
                self.rendered[index] = text;
            }
        }
        Ok(())
    }
}

fn format_duration(milliseconds: u64, tenths: bool) -> String {
    let seconds = milliseconds / 1000;
    let mut text = format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    );
    if tenths {
        text.push_str(&format!(".{}", milliseconds % 1000 / 100));
    }
    text
}
