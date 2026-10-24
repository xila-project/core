embedded_i18n::track_locales!();

#[cfg(target_arch = "wasm32")]
mod install;
#[cfg(target_arch = "wasm32")]
mod interface;

#[cfg(target_arch = "wasm32")]
fn main() {
    if let Err(error) = interface::run() {
        eprintln!("Clock: graphics error {error}");
        std::process::exit(1);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    println!("Clock is a Xila Wasm application. Build for wasm32-wasip1.");
}

#[cfg(test)]
mod localization_tests {
    use core::fmt::Write;

    fn lap_label(count: usize) -> String {
        let mut output = String::new();
        embedded_i18n::translate_plural_write!(&mut output, "Recorded laps", count)
            .expect("writing to String cannot fail");
        output
    }

    #[test]
    fn stopwatch_lap_plural_matches_selected_locale() {
        let (one, other) = match embedded_i18n::get_locale() {
            "fr" => ("1 tour enregistré", "2 tours enregistrés"),
            "zh-CN" => ("已记录 1 次", "已记录 2 次"),
            _ => ("1 lap recorded", "2 laps recorded"),
        };
        assert_eq!(lap_label(1), one);
        assert_eq!(lap_label(2), other);
    }
}
