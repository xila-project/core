#[cfg(test)]
mod tests {
    #[test]
    fn generated_font_symbols_link() {
        let fonts = [
            graphics::fonts::get_font_small() as *const _,
            graphics::fonts::get_font_medium() as *const _,
            graphics::fonts::get_font_large() as *const _,
            graphics::fonts::get_font_extra_large() as *const _,
            graphics::fonts::get_font_monospace_small() as *const _,
            graphics::fonts::get_font_monospace_medium() as *const _,
        ];

        std::hint::black_box(fonts);
    }
}
