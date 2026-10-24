use crate::engine::evaluate_expression;
use embedded_i18n::translate;
use std::{ptr::null_mut, thread::sleep};
use wasm::{
    self, Color, EVENT_CLOSE_REQUESTED, EventCode, FlexFlow, Object, ObjectFlag,
    buttonmatrix_create, buttonmatrix_get_selected_button, buttonmatrix_set_map, label_create,
    label_set_text, object_add_flag, object_create, object_set_flex_flow, object_set_flex_grow,
    object_set_height, object_set_width, percentage, size_content, window_create, window_pop_event,
    window_set_icon,
};

#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)] // Indirectly constructed with from_u8
#[repr(u8)]
pub enum ButtonIdentifier {
    // Row 1 (indices 0-6)
    DegRad,           // "DEG/RAD"
    LeftParenthesis,  // "("
    RightParenthesis, // ")"
    Clear,            // "C"
    Backspace,        // "<"
    Factorial,        // "x!"
    Divide,           // "/"
    Abs,              // "|x|"

    // Row 2 (indices 7-13)
    Sin,      // "sin"
    Cos,      // "cos"
    Tan,      // "tan"
    Seven,    // "7"
    Eight,    // "8"
    Nine,     // "9"
    Multiply, // "*"
    Percent,  // "%"

    // Row 3 (indices 14-20)
    Sinh,     // "sinh"
    Cosh,     // "cosh"
    Tanh,     // "tanh"
    Four,     // "4"
    Five,     // "5"
    Six,      // "6"
    Subtract, // "-"
    Mod,      // "mod"

    // Row 4 (indices 21-27)
    Ln,     // "ln"
    Log10,  // "log"
    Sqrt,   // "sqrt"
    One,    // "1"
    Two,    // "2"
    Three,  // "3"
    Add,    // "+"
    Random, // "rand"

    // Row 5 (indices 28-41)
    Power,    // "^"
    Square,   // "x²"
    Cube,     // "x³"
    TenPower, // "10^x"
    Zero,     // "0"
    Decimal,  // "."
    Equals,   // "="
    Inverse,  // "1/x"
    Pi,       // "pi"
    E,        // "e"
}

const BUTTON_MAP: [*const i8; 48] = [
    // Row 1: Clear, Backspace, Angle mode, Parentheses, Division, Factorial, Abs
    translate!(c"angle_mode.radians").as_ptr(), // Angle mode toggle
    c"(".as_ptr(),
    c")".as_ptr(),
    translate!(c"controls.clear").as_ptr(),
    c"<".as_ptr(), // Backspace
    c"x!".as_ptr(),
    c"/".as_ptr(),
    c"|x|".as_ptr(), // Absolute value
    c"\n".as_ptr(),
    // Row 2: Basic trig functions + 7,8,9,*, %
    c"sin".as_ptr(),
    c"cos".as_ptr(),
    c"tan".as_ptr(),
    c"7".as_ptr(),
    c"8".as_ptr(),
    c"9".as_ptr(),
    c"*".as_ptr(),
    c"%".as_ptr(), // Percent
    c"\n".as_ptr(),
    // Row 3: Hyperbolic functions + 4,5,6,-, mod
    c"sinh".as_ptr(),
    c"cosh".as_ptr(),
    c"tanh".as_ptr(),
    c"4".as_ptr(),
    c"5".as_ptr(),
    c"6".as_ptr(),
    c"-".as_ptr(),
    c"mod".as_ptr(), // Modulo
    c"\n".as_ptr(),
    // Row 4: Logarithmic functions + 1,2,3,+, rand
    c"ln".as_ptr(),
    c"log".as_ptr(),
    c"sqrt".as_ptr(),
    c"1".as_ptr(),
    c"2".as_ptr(),
    c"3".as_ptr(),
    c"+".as_ptr(),
    c"rand".as_ptr(), // Random
    c"\n".as_ptr(),
    // Row 5: Power functions + 0, decimal, equals, 10^x, inverse, constants
    c"^".as_ptr(),
    c"x^2".as_ptr(),
    c"x^3".as_ptr(),
    c"10^x".as_ptr(),
    c"0".as_ptr(),
    c".".as_ptr(),
    c"=".as_ptr(),
    c"1/x".as_ptr(),
    c"pi".as_ptr(),
    c"e".as_ptr(),
    c"\n".as_ptr(),
    null_mut(), // Extra null to fill the 48-button map
];

impl ButtonIdentifier {
    pub const fn to_u8(self) -> u8 {
        self as u8
    }

    pub const fn from_u8(identifier: u8) -> Option<Self> {
        if identifier >= ButtonIdentifier::E.to_u8() {
            return None; // Invalid button ID
        }
        unsafe { Some(std::mem::transmute::<u8, ButtonIdentifier>(identifier)) }
    }
}

pub struct Interface {
    window: *mut Object,
    _display: *mut Object,
    display_label: *mut Object,
    button_matrix: *mut Object,
    current_expression: String,
    show_result: bool,
    is_radian_mode: bool, // true for radians, false for degrees
}

impl Interface {
    pub fn new() -> wasm::Result<Self> {
        let window = unsafe { Self::create_window() }?;

        let (display, display_label) = unsafe { Self::create_display(window) }?;

        let button_matrix = unsafe { Self::create_button_matrix(window)? };

        Ok(Self {
            window,
            _display: display,
            display_label,
            button_matrix,
            current_expression: String::new(),
            show_result: false,
            is_radian_mode: true, // Default to radians
        })
    }

    unsafe fn create_window() -> wasm::Result<*mut Object> {
        // Create main window
        unsafe {
            let window = window_create()?;

            window_set_icon(
                window,
                c"Ca".as_ptr(),
                Color {
                    red: 0,
                    green: 0,
                    blue: 0,
                },
            )?;

            object_set_flex_flow(window, FlexFlow::Column)?;

            Ok(window)
        }
    }

    unsafe fn create_display(window: *mut Object) -> wasm::Result<(*mut Object, *mut Object)> {
        unsafe {
            // Create display container using generic object create
            let display = object_create(window)?;
            // Note: Skip styling for now due to color type issues

            // Create display label
            let display_label = label_create(display)?;
            object_set_height(display, size_content())?; // Set height to content size
            label_set_text(display_label, c"0".as_ptr() as *mut _)?;

            // Note: Skip text styling for now

            let width: i32 = percentage(100)?;

            object_set_width(display, width)?;

            Ok((display, display_label))
        }
    }

    unsafe fn create_button_matrix(window: *mut Object) -> wasm::Result<*mut Object> {
        // Create button matrix for calculator
        unsafe {
            let button_matrix = buttonmatrix_create(window)?;

            object_set_height(button_matrix, size_content())?;

            object_set_flex_grow(button_matrix, 1)?; /*1 portion from the free space*/

            object_add_flag(button_matrix, ObjectFlag::EventBubble)?;

            let width: i32 = percentage(100)?;
            object_set_width(button_matrix, width)?;

            // Position the button matrix below the display

            // Define button layout - Extended scientific calculator

            // Set the button map
            buttonmatrix_set_map(button_matrix, BUTTON_MAP.as_ptr())?;

            // Optional: Make some buttons wider if needed
            // buttonmatrix_set_button_width(self.button_matrix, 31, 2); // Make "0" wider

            Ok(button_matrix)
        }
    }

    fn update_display(&mut self) -> wasm::Result<()> {
        unsafe {
            let display_text = if self.show_result {
                self.current_expression.clone()
            } else if self.current_expression.is_empty() {
                "0".to_string()
            } else {
                self.current_expression.clone()
            };

            // Add angle mode indicator
            let mode_indicator = if self.is_radian_mode {
                translate!("angle_mode.radians_indicator")
            } else {
                translate!("angle_mode.degrees_indicator")
            };
            let full_text = format!("{}{}", display_text, mode_indicator);

            let text_with_null = format!("{}\0", full_text);
            label_set_text(self.display_label, text_with_null.as_ptr() as *mut _)?;
        }

        Ok(())
    }

    unsafe fn handle_button_matrix_event(&mut self) -> wasm::Result<()> {
        // Get the selected button ID from the button matrix
        let selected_button_id: u32 =
            unsafe { buttonmatrix_get_selected_button(self.button_matrix) }?;

        // Convert button ID to enum and handle
        if let Some(button) = ButtonIdentifier::from_u8(selected_button_id as u8) {
            self.handle_button_press(button)?;
        }
        Ok(())
    }

    fn handle_button_press(&mut self, button: ButtonIdentifier) -> wasm::Result<()> {
        match button {
            ButtonIdentifier::Clear => {
                self.current_expression.clear();
                self.show_result = false;
            }
            ButtonIdentifier::Backspace => {
                if !self.current_expression.is_empty() {
                    self.current_expression.pop();
                }
                self.show_result = false;
            }
            ButtonIdentifier::DegRad => {
                self.is_radian_mode = !self.is_radian_mode;
                // Update the display to show current mode
                self.update_display()?;
                return Ok(()); // Don't call update_display again at the end
            }
            ButtonIdentifier::Equals => {
                if !self.current_expression.is_empty() {
                    match evaluate_expression(&self.current_expression, self.is_radian_mode) {
                        Ok(result) => {
                            self.current_expression = result.to_string();
                            self.show_result = true;
                        }
                        Err(e) => {
                            self.current_expression =
                                format!(translate!("errors.calculation_with_details"), e);
                            self.show_result = true;
                        }
                    }
                }
            }
            // Basic trigonometric functions
            ButtonIdentifier::Sin => self.push_str("sin("),
            ButtonIdentifier::Cos => self.push_str("cos("),
            ButtonIdentifier::Tan => self.push_str("tan("),

            // Hyperbolic functions
            ButtonIdentifier::Sinh => self.push_str("sinh("),
            ButtonIdentifier::Cosh => self.push_str("cosh("),
            ButtonIdentifier::Tanh => self.push_str("tanh("),

            // Logarithmic functions
            ButtonIdentifier::Ln => self.push_str("ln("),
            ButtonIdentifier::Log10 => self.push_str("log("),

            // Root and power functions
            ButtonIdentifier::Sqrt => self.push_str("sqrt("),
            ButtonIdentifier::Square => self.push_str("sqr("),
            ButtonIdentifier::Cube => self.push_str("cube("),
            ButtonIdentifier::TenPower => self.push_str("pow10("),

            // Other functions
            ButtonIdentifier::Factorial => self.push_char('!'),
            ButtonIdentifier::Inverse => self.push_str("inv("),
            ButtonIdentifier::Abs => self.push_str("abs("),
            ButtonIdentifier::Random => {
                // Generate random number immediately and insert it
                use std::collections::hash_map::DefaultHasher;
                use std::hash::{Hash, Hasher};

                let mut hasher = DefaultHasher::new();
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
                    .hash(&mut hasher);
                let hash = hasher.finish();
                let random_value = (hash as f64) / (u64::MAX as f64);
                self.push_str(&format!("{:.10}", random_value));
                return Ok(()); // Don't call update_display again at the end
            }

            // Constants
            ButtonIdentifier::Pi => self.push_str("pi"),
            ButtonIdentifier::E => self.push_str("e"),

            // Parentheses and operators
            ButtonIdentifier::LeftParenthesis => self.push_str("("),
            ButtonIdentifier::RightParenthesis => self.push_char(')'),
            ButtonIdentifier::Decimal => self.push_str("."),

            // Operator buttons that continue the expression
            ButtonIdentifier::Power => self.push_operator('^'),
            ButtonIdentifier::Divide => self.push_operator('/'),
            ButtonIdentifier::Multiply => self.push_operator('*'),
            ButtonIdentifier::Subtract => self.push_operator('-'),
            ButtonIdentifier::Add => self.push_operator('+'),
            ButtonIdentifier::Percent => self.push_operator('%'),
            ButtonIdentifier::Mod => self.push_str(" mod "),

            // Number buttons
            ButtonIdentifier::Zero => self.push_char('0'),
            ButtonIdentifier::One => self.push_char('1'),
            ButtonIdentifier::Two => self.push_char('2'),
            ButtonIdentifier::Three => self.push_char('3'),
            ButtonIdentifier::Four => self.push_char('4'),
            ButtonIdentifier::Five => self.push_char('5'),
            ButtonIdentifier::Six => self.push_char('6'),
            ButtonIdentifier::Seven => self.push_char('7'),
            ButtonIdentifier::Eight => self.push_char('8'),
            ButtonIdentifier::Nine => self.push_char('9'),
        }

        // Update display after any button press except Clear, Backspace, DegRad, and Equals
        self.update_display()?;

        Ok(())
    }

    fn push_str(&mut self, text: &str) {
        if self.show_result {
            self.current_expression.clear();
            self.show_result = false;
        }
        self.current_expression.push_str(text);
    }

    fn push_char(&mut self, ch: char) {
        if self.show_result {
            self.current_expression.clear();
            self.show_result = false;
        }
        self.current_expression.push(ch);
    }

    fn push_operator(&mut self, op: char) {
        // For operators, if showing result, keep it and append the operator
        if self.show_result {
            self.show_result = false;
        }
        self.current_expression.push(op);
    }

    pub unsafe fn run(&mut self) -> wasm::Result<()> {
        // Create GUI components
        // Initial display update
        self.update_display()?;

        // Main event loop
        loop {
            let mut code = EventCode::All as u32;
            let mut target: *mut Object = null_mut();

            unsafe {
                window_pop_event(
                    self.window,
                    &mut code as *mut _ as *mut _,
                    &mut target as *mut _ as *mut _,
                )?;
            }

            if code == EVENT_CLOSE_REQUESTED {
                return Ok(());
            } else if code == EventCode::Clicked as u32 {
                // Check if the event came from our button matrix
                if target == self.button_matrix {
                    unsafe {
                        self.handle_button_matrix_event()?;
                    }
                }
            } else if code == EventCode::All as u32 {
                sleep(std::time::Duration::from_millis(10));
            }
        }
    }
}
