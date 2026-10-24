use embedded_i18n::translate;
use nanoexpr::{Context, ErrorKind, Evaluator, Exact, Functions, Math, Number, PercentSign};

type CalculatorNumber = Exact<14, 99>;

pub fn evaluate_expression(input: &str, radian_mode: bool) -> Result<CalculatorNumber, String> {
    let functions = Functions(|name: &str, arguments: &[CalculatorNumber]| match name {
        "sqr" | "cube" | "pow10" | "inv" | "sin" | "cos" | "tan" if arguments.len() != 1 => {
            Some(Err(ErrorKind::ArgumentCount))
        }
        "sqr" | "cube" | "pow10" | "inv" | "sin" | "cos" | "tan" => {
            let value = arguments[0];
            match name {
                "sqr" => Some(value.multiply(value)),
                "cube" => Some(
                    value
                        .multiply(value)
                        .and_then(|square| square.multiply(value)),
                ),
                "pow10" => {
                    CalculatorNumber::parse("10").and_then(|ten| Math.call("pow", &[ten, value]))
                }
                "inv" => Some(CalculatorNumber::from_bool(true).and_then(|one| one.divide(value))),
                "sin" | "cos" | "tan" if !radian_mode => {
                    let pi: CalculatorNumber =
                        Math.variable("pi").expect("nanoexpr Math defines pi");
                    let degrees_to_radians =
                        pi.divide(CalculatorNumber::parse("180").expect("180 is representable"));
                    degrees_to_radians
                        .and_then(|factor| value.multiply(factor))
                        .ok()
                        .and_then(|radians| Math.call(name, &[radians]))
                }
                "sin" | "cos" | "tan" => None,
                _ => None,
            }
        }
        _ => None,
    });
    let context = (functions, Math);
    Evaluator::<_, 4>::new(&context)
        .percent_sign(PercentSign::Percentage)
        .implicit_multiplication(true)
        .evaluate::<CalculatorNumber>(input)
        .map_err(localize_error)
}

fn localize_error(error: nanoexpr::Error) -> String {
    let message = match error.kind {
        ErrorKind::UnexpectedCharacter => translate!("Unexpected character"),
        ErrorKind::UnexpectedToken => translate!("Unexpected token"),
        ErrorKind::UnexpectedEnd => translate!("Unexpected end of expression"),
        ErrorKind::UnclosedParenthesis => translate!("Unclosed parenthesis"),
        ErrorKind::InvalidNumber => translate!("Invalid number"),
        ErrorKind::UnknownVariable => translate!("Unknown variable"),
        ErrorKind::UnknownFunction => translate!("Unknown function"),
        ErrorKind::TooManyArguments => translate!("Too many arguments"),
        ErrorKind::ArgumentCount => translate!("Wrong number of arguments"),
        ErrorKind::DivisionByZero => translate!("Division by zero"),
        ErrorKind::Domain => translate!("Value outside function domain"),
        ErrorKind::Overflow => translate!("Calculation overflow"),
        ErrorKind::TooDeep => translate!("Expression nested too deeply"),
        ErrorKind::Unsupported => translate!("Unsupported operation"),
        ErrorKind::TooLong => translate!("Expression too long"),
        ErrorKind::Inexact => translate!("Inexact result"),
        _ => translate!("Calculation error"),
    };
    format!("{message} ({}..{})", error.span.start, error.span.end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_exact_fraction_results() {
        assert_eq!(
            evaluate_expression("0.1 + 0.2", true).unwrap().to_string(),
            "3/10"
        );
        assert_eq!(
            evaluate_expression("1 / 3 + 1 / 6", true)
                .unwrap()
                .to_string(),
            "1/2"
        );
        assert_eq!(evaluate_expression("5!", true).unwrap().to_string(), "120");
    }

    #[test]
    fn preserves_calculator_function_buttons_and_percent() {
        assert_eq!(
            evaluate_expression("sqr(5)", true).unwrap().to_string(),
            "25"
        );
        assert_eq!(
            evaluate_expression("cube(3)", true).unwrap().to_string(),
            "27"
        );
        assert_eq!(
            evaluate_expression("pow10(2)", true).unwrap().to_string(),
            "100"
        );
        assert_eq!(
            evaluate_expression("inv(4)", true).unwrap().to_string(),
            "1/4"
        );
        assert_eq!(evaluate_expression("25%", true).unwrap().to_string(), "1/4");
    }

    #[test]
    fn falls_back_to_ti_precision_decimal_for_irrational_results() {
        let result = evaluate_expression("sqrt(2)", true).unwrap();
        assert!(!result.is_exact());
        assert_eq!(result.to_string(), "1.4142135623731");
    }

    #[test]
    fn converts_degrees_without_losing_decimal_precision() {
        let result = evaluate_expression("sin(90)", false).unwrap();
        assert_eq!(result.to_string(), "1");
    }

    #[test]
    fn reports_localized_errors_with_the_input_span() {
        assert_eq!(
            evaluate_expression("1 / 0", true).unwrap_err(),
            format!("{} (0..5)", translate!("Division by zero"))
        );
        assert_eq!(
            evaluate_expression("sqrt(-1)", true).unwrap_err(),
            format!("{} (0..8)", translate!("Value outside function domain"))
        );
    }
}
