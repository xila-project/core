use clock::{Alarm, AlarmAlert, Settings, Stopwatch, Timer, storage::Store};
use embedded_i18n::translate;
use std::{
    ffi::{CStr, CString},
    ptr::null_mut,
    thread::sleep,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use wasm::*;

const SETTINGS_DIRECTORY: &str = "/configuration/shared/clock";

const fn color(red: u8, green: u8, blue: u8) -> Color {
    Color { red, green, blue }
}

#[derive(Clone, Copy)]
enum Action {
    Format,
    AlarmTimeChanged,
    AlarmSave,
    AlarmDisable,
    AlarmDelete,
    StopwatchToggle,
    Lap,
    StopwatchReset,
    TimerDurationChanged,
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
    alert_panel: *mut Object,
    alert_snooze: *mut Object,
    alert_dismiss: *mut Object,
    controls: Vec<(*mut Object, Action)>,
    labels: [*mut Object; 8],
    rendered: [String; 8],
    weekdays: [*mut Object; 7],
    alarm_hour_input: *mut Object,
    alarm_minute_input: *mut Object,
    timer_minute_input: *mut Object,
    timer_second_input: *mut Object,
    timer_arc: *mut Object,
    timer_started: bool,
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
            if (code == EventCode::Clicked as u32 || code == EventCode::ValueChanged as u32)
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
            let (store, settings, persistence_error) = match Store::load(SETTINGS_DIRECTORY) {
                Ok((store, settings)) => (Some(store), settings, false),
                Err(error) => {
                    eprintln!("Clock settings unavailable: {error}");
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
            Self::surface(window, color(0, 0, 0), color(0, 0, 0), 0, 0)?;
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
                alert_panel: null_mut(),
                alert_snooze: null_mut(),
                alert_dismiss: null_mut(),
                controls: Vec::with_capacity(32),
                labels: [null_mut(); 8],
                rendered: core::array::from_fn(|_| String::new()),
                weekdays: [null_mut(); 7],
                alarm_hour_input: null_mut(),
                alarm_minute_input: null_mut(),
                timer_minute_input: null_mut(),
                timer_second_input: null_mut(),
                timer_arc: null_mut(),
                timer_started: false,
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
            let alert = ui.card(window)?;
            object_add_flag(alert, ObjectFlag::Hidden)?;
            ui.alert_panel = alert;
            ui.labels[0] = ui.label(alert, "")?;
            let alert_row = ui.row(alert)?;
            ui.alert_snooze = ui.button(alert_row, translate!(c"Snooze 5 min"), Action::Snooze)?;
            ui.alert_dismiss = ui.button(alert_row, translate!(c"Dismiss"), Action::Dismiss)?;

            let tabs = tabview_create(window)?;
            object_set_width(tabs, percentage(100)?)?;
            object_set_flex_grow(tabs, 1)?;
            Self::surface(tabs, color(0, 0, 0), color(0, 0, 0), 0, 0)?;
            ui.tabs = tabs;
            let clock_tab = ui.tab(translate!(c"Clock"))?;
            let clock_card = ui.card(clock_tab)?;
            ui.label(clock_card, translate!("Current time"))?;
            ui.labels[1] = ui.label(clock_card, "")?;
            object_set_height(ui.labels[1], 112)?;
            object_set_style_transform_scale_x(ui.labels[1], 768, PART_MAIN)?;
            object_set_style_transform_scale_y(ui.labels[1], 768, PART_MAIN)?;
            object_set_style_text_align(ui.labels[1], TextAlign::Center, PART_MAIN)?;
            object_set_style_text_color(ui.labels[1], color(255, 255, 255), PART_MAIN)?;
            ui.labels[2] = ui.label(clock_card, "")?;
            object_set_style_text_color(ui.labels[2], color(142, 142, 147), PART_MAIN)?;
            ui.label(clock_card, translate!("UTC / host clock"))?;
            let clock_actions = ui.row(clock_card)?;
            ui.button(clock_actions, translate!(c"12 / 24 hour"), Action::Format)?;

            let alarm_tab = ui.tab(translate!(c"Alarm"))?;
            let alarm_card = ui.card(alarm_tab)?;
            ui.label(alarm_card, translate!("Alarm time"))?;
            let alarm_row = ui.row(alarm_card)?;
            ui.alarm_hour_input = ui.spinbox(alarm_row, 0, 23, 2, ui.alarm_hour as i32)?;
            ui.inline_label(alarm_row, ":")?;
            ui.alarm_minute_input = ui.spinbox(alarm_row, 0, 59, 2, ui.alarm_minute as i32)?;
            ui.controls
                .push((ui.alarm_hour_input, Action::AlarmTimeChanged));
            ui.controls
                .push((ui.alarm_minute_input, Action::AlarmTimeChanged));
            ui.labels[3] = ui.label(alarm_card, translate!("No saved alarm"))?;
            ui.label(
                alarm_card,
                translate!("Repeat on selected days; none means once. All times are UTC."),
            )?;
            let days = [
                translate!(c"Mon"),
                translate!(c"Tue"),
                translate!(c"Wed"),
                translate!(c"Thu"),
                translate!(c"Fri"),
                translate!(c"Sat"),
                translate!(c"Sun"),
            ];
            let day_row = ui.row(alarm_card)?;
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
            let alarm_actions = ui.row(alarm_card)?;
            ui.button(
                alarm_actions,
                translate!(c"Save and enable"),
                Action::AlarmSave,
            )?;
            ui.button(alarm_actions, translate!(c"Disable"), Action::AlarmDisable)?;
            ui.button(alarm_actions, translate!(c"Delete"), Action::AlarmDelete)?;

            let watch_tab = ui.tab(translate!(c"Stopwatch"))?;
            let watch_card = ui.card(watch_tab)?;
            ui.label(watch_card, translate!("Stopwatch"))?;
            ui.labels[4] = ui.label(watch_card, "")?;
            object_set_height(ui.labels[4], 72)?;
            object_set_style_transform_scale_x(ui.labels[4], 640, PART_MAIN)?;
            object_set_style_transform_scale_y(ui.labels[4], 640, PART_MAIN)?;
            object_set_style_text_align(ui.labels[4], TextAlign::Center, PART_MAIN)?;
            let watch_actions = ui.row(watch_card)?;
            ui.button(
                watch_actions,
                translate!(c"Start / pause"),
                Action::StopwatchToggle,
            )?;
            ui.button(watch_actions, translate!(c"Lap"), Action::Lap)?;
            ui.button(watch_actions, translate!(c"Reset"), Action::StopwatchReset)?;
            ui.labels[5] = ui.label(watch_card, "")?;

            let timer_tab = ui.tab(translate!(c"Timer"))?;
            let timer_card = ui.card(timer_tab)?;
            ui.label(timer_card, translate!("Timer"))?;
            let timer_content = ui.row(timer_card)?;
            let timer_dial = object_create(timer_content)?;
            object_set_width(timer_dial, 180)?;
            object_set_height(timer_dial, 180)?;
            ui.timer_arc = arc_create(timer_dial)?;
            object_set_width(ui.timer_arc, 180)?;
            object_set_height(ui.timer_arc, 180)?;
            object_align(ui.timer_arc, Align::Center, 0, 0)?;
            arc_set_range(ui.timer_arc, 0, 100)?;
            arc_set_value(ui.timer_arc, 100)?;
            arc_set_rotation(ui.timer_arc, 135)?;
            object_remove_flag(ui.timer_arc, ObjectFlag::Clickable)?;
            object_set_style_arc_color(ui.timer_arc, color(52, 58, 69), PART_MAIN)?;
            object_set_style_arc_width(ui.timer_arc, 12, PART_MAIN)?;
            object_set_style_arc_color(ui.timer_arc, color(59, 130, 246), PART_INDICATOR)?;
            object_set_style_arc_width(ui.timer_arc, 12, PART_INDICATOR)?;
            ui.labels[6] = ui.inline_label(timer_dial, "")?;
            object_set_width(ui.labels[6], 140)?;
            object_align(ui.labels[6], Align::Center, 0, 0)?;
            object_set_style_transform_scale_x(ui.labels[6], 512, PART_MAIN)?;
            object_set_style_transform_scale_y(ui.labels[6], 512, PART_MAIN)?;
            object_set_style_text_align(ui.labels[6], TextAlign::Center, PART_MAIN)?;
            ui.labels[7] = ui.label(timer_card, translate!("Duration"))?;
            let duration_row = ui.row(timer_card)?;
            ui.timer_minute_input = ui.spinbox(duration_row, 0, 1440, 4, 5)?;
            ui.inline_label(duration_row, translate!("min"))?;
            ui.timer_second_input = ui.spinbox(duration_row, 0, 59, 2, 0)?;
            ui.inline_label(duration_row, translate!("sec"))?;
            ui.controls
                .push((ui.timer_minute_input, Action::TimerDurationChanged));
            ui.controls
                .push((ui.timer_second_input, Action::TimerDurationChanged));
            let timer_actions = ui.row(timer_card)?;
            ui.button(timer_actions, translate!(c"Start"), Action::TimerStart)?;
            ui.button(
                timer_actions,
                translate!(c"Pause / resume"),
                Action::TimerToggle,
            )?;
            ui.button(timer_actions, translate!(c"Restart"), Action::TimerRestart)?;
            ui.button(timer_actions, translate!(c"Cancel"), Action::TimerCancel)?;
            Ok(ui)
        }
    }

    unsafe fn label(&self, parent: *mut Object, text: &str) -> wasm::Result<*mut Object> {
        unsafe {
            let label = label_create(parent)?;
            object_set_width(label, percentage(100)?)?;
            object_set_style_text_color(label, color(224, 227, 234), PART_MAIN)?;
            Self::text(label, text)?;
            Ok(label)
        }
    }
    unsafe fn inline_label(&self, parent: *mut Object, text: &str) -> wasm::Result<*mut Object> {
        unsafe {
            let label = label_create(parent)?;
            object_set_style_text_color(label, color(224, 227, 234), PART_MAIN)?;
            Self::text(label, text)?;
            Ok(label)
        }
    }
    unsafe fn card(&self, parent: *mut Object) -> wasm::Result<*mut Object> {
        unsafe {
            let card = object_create(parent)?;
            object_set_width(card, percentage(100)?)?;
            object_set_height(card, size_content())?;
            object_set_flex_flow(card, FlexFlow::Column)?;
            object_set_flex_align(
                card,
                FlexAlign::Center,
                FlexAlign::Center,
                FlexAlign::Center,
            )?;
            Self::surface(card, color(28, 28, 30), color(58, 58, 60), 16, 18)?;
            Ok(card)
        }
    }
    unsafe fn surface(
        object: *mut Object,
        background: Color,
        border: Color,
        radius: i32,
        padding: i32,
    ) -> wasm::Result<()> {
        unsafe {
            object_set_style_bg_color(object, background, PART_MAIN)?;
            object_set_style_bg_opa(object, 255, PART_MAIN)?;
            object_set_style_border_color(object, border, PART_MAIN)?;
            object_set_style_border_width(object, i32::from(radius > 0), PART_MAIN)?;
            object_set_style_radius(object, radius, PART_MAIN)?;
            object_set_style_pad_left(object, padding, PART_MAIN)?;
            object_set_style_pad_right(object, padding, PART_MAIN)?;
            object_set_style_pad_top(object, padding, PART_MAIN)?;
            object_set_style_pad_bottom(object, padding, PART_MAIN)?;
            object_set_style_pad_row(object, 14, PART_MAIN)?;
            Ok(())
        }
    }
    unsafe fn spinbox(
        &self,
        parent: *mut Object,
        min: i32,
        max: i32,
        digits: u32,
        value: i32,
    ) -> wasm::Result<*mut Object> {
        unsafe {
            let input = spinbox_create(parent)?;
            spinbox_set_digit_format(input, digits, 0)?;
            spinbox_set_range(input, min, max)?;
            spinbox_set_rollover(input, true)?;
            spinbox_set_step(input, 1)?;
            spinbox_set_value(input, value)?;
            object_set_width(input, 110)?;
            object_set_height(input, 44)?;
            object_add_flag(input, ObjectFlag::EventBubble)?;
            Ok(input)
        }
    }
    unsafe fn text(label: *mut Object, text: &str) -> wasm::Result<()> {
        let text = CString::new(text).map_err(|_| -1)?;
        unsafe { label_set_text(label, text.as_ptr()) }
    }
    unsafe fn set_hidden(object: *mut Object, hidden: bool) -> wasm::Result<()> {
        unsafe {
            if hidden {
                object_add_flag(object, ObjectFlag::Hidden)
            } else {
                object_remove_flag(object, ObjectFlag::Hidden)
            }
        }
    }
    unsafe fn row(&self, parent: *mut Object) -> wasm::Result<*mut Object> {
        unsafe {
            let row = object_create(parent)?;
            object_set_width(row, percentage(100)?)?;
            object_set_height(row, size_content())?;
            object_set_flex_flow(row, FlexFlow::RowWrap)?;
            object_set_flex_align(row, FlexAlign::Center, FlexAlign::Center, FlexAlign::Center)?;
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
    ) -> wasm::Result<*mut Object> {
        unsafe {
            let button = button_create(parent)?;
            object_set_style_bg_color(button, color(255, 159, 10), PART_MAIN)?;
            object_set_style_bg_opa(button, 255, PART_MAIN)?;
            object_set_style_border_width(button, 0, PART_MAIN)?;
            object_set_style_radius(button, 10, PART_MAIN)?;
            object_set_style_pad_left(button, 16, PART_MAIN)?;
            object_set_style_pad_right(button, 16, PART_MAIN)?;
            let label = label_create(button)?;
            label_set_text(label, text.as_ptr())?;
            // Window events identify the control rather than its child label.
            object_add_flag(button, ObjectFlag::EventBubble)?;
            self.controls.push((button, action));
            Ok(button)
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
                Action::AlarmTimeChanged => {
                    self.alarm_hour = spinbox_get_value(self.alarm_hour_input)?.clamp(0, 23) as u8;
                    self.alarm_minute =
                        spinbox_get_value(self.alarm_minute_input)?.clamp(0, 59) as u8;
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
                Action::TimerDurationChanged => {
                    let minutes = spinbox_get_value(self.timer_minute_input)?.clamp(0, 1440);
                    let seconds = spinbox_get_value(self.timer_second_input)?.clamp(0, 59);
                    self.timer_seconds = (minutes * 60 + seconds).clamp(1, 86400) as u32;
                }
                Action::TimerStart => {
                    self.timer.start(u64::from(self.timer_seconds) * 1000, now);
                    self.timer_started = true;
                }
                Action::TimerToggle => {
                    if self.timer.is_running() {
                        self.timer.pause(now);
                    } else if self.timer_started {
                        self.timer.resume(now);
                    }
                }
                Action::TimerRestart => {
                    if self.timer_started {
                        self.timer.restart(now);
                    }
                }
                Action::TimerCancel => {
                    self.timer.cancel();
                    self.timer_started = false;
                }
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
            alert.push_str(translate!("Alarm! "));
        }
        if self.timer.finished() {
            alert.push_str(translate!("Timer finished! "));
        }
        if self.persistence_error {
            alert.push_str(translate!("Settings could not be saved. "));
        }
        unsafe {
            Self::set_hidden(self.alert_panel, alert.is_empty())?;
            Self::set_hidden(self.alert_snooze, !self.alarm_alert.is_ringing())?;
            Self::set_hidden(
                self.alert_dismiss,
                !self.alarm_alert.is_ringing() && !self.timer.finished(),
            )?;
        }
        let (clock, date) = wall.and_then(|wall| i64::try_from(wall).ok()).map_or_else(
            || {
                (
                    translate!("Host clock unavailable").to_string(),
                    String::new(),
                )
            },
            |wall| {
                let mut clock = String::new();
                let mut date = String::new();
                let time_pattern = if self.settings.twelve_hour {
                    "%I:%M:%S %p"
                } else {
                    "%H:%M:%S"
                };
                let _ = embedded_i18n::write_unix_timestamp(&mut clock, wall, time_pattern);
                let _ = embedded_i18n::write_unix_timestamp(&mut date, wall, "%a, %d %b %Y");
                (clock, date)
            },
        );
        let mut alarm = String::new();
        if active == 1 {
            if let Some(saved) = self.settings.alarm {
                if saved.enabled() {
                    alarm.push_str(translate!("Alarm enabled: "));
                    alarm.push_str(&format!("{:02}:{:02} UTC", saved.hour, saved.minute));
                    if let Some(next) = saved.next().and_then(|value| i64::try_from(value).ok()) {
                        alarm.push('\n');
                        alarm.push_str(translate!("Next: "));
                        let _ = embedded_i18n::write_unix_timestamp(
                            &mut alarm,
                            next,
                            "%a, %d %b %H:%M",
                        );
                    }
                } else {
                    alarm.push_str(translate!("Disabled"));
                }
            } else {
                alarm.push_str(translate!("No saved alarm"));
            }
            if self.alarm_alert.is_snoozed() {
                alarm.push('\n');
                alarm.push_str(translate!("Alarm snoozed"));
            }
            alarm.push_str(&format!(
                "\n{:02}:{:02} UTC",
                self.alarm_hour, self.alarm_minute
            ));
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
                let _ = embedded_i18n::translate_plural_write!(&mut laps, "Recorded laps", count);
                laps.push('\n');
            }
            for (index, lap) in self.watch.laps().iter().enumerate() {
                let _ = writeln!(laps, "{}: {}", index + 1, format_duration(*lap, true));
            }
        }
        let (timer, timer_duration) = if active == 3 {
            let duration = u64::from(self.timer_seconds) * 1000;
            let remaining = if self.timer_started {
                self.timer.remaining(now)
            } else {
                duration
            };
            let progress = if self.timer_started && self.timer_seconds != 0 {
                (remaining.div_ceil(1000) * 100 / u64::from(self.timer_seconds)).min(100) as i32
            } else {
                100
            };
            unsafe {
                arc_set_value(self.timer_arc, progress)?;
            }
            (
                format_duration(remaining.div_ceil(1000) * 1000, false),
                format!(
                    "{}: {}",
                    translate!("Duration"),
                    format_duration(duration, false)
                ),
            )
        } else {
            (String::new(), String::new())
        };
        for (index, text) in [
            alert,
            clock,
            date,
            alarm,
            watch,
            laps,
            timer,
            timer_duration,
        ]
        .into_iter()
        .enumerate()
        {
            if !matches!(
                (active, index),
                (_, 0) | (0, 1 | 2) | (1, 3) | (2, 4 | 5) | (3, 6 | 7)
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
