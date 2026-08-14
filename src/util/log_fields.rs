pub fn number(value: Option<f64>) -> String {
    match value {
        Some(value) if value.is_finite() => format!("{value}"),
        Some(value) => format!("NOT FINITE: {value}"),
        None => "unset".to_string(),
    }
}

pub fn span(value: Option<(f64, f64)>) -> String {
    match value {
        Some((low, high)) => format!("{} to {}", number(Some(low)), number(Some(high))),
        None => "none".to_string(),
    }
}

pub fn count(value: Option<u32>) -> String {
    value.map_or_else(|| "unset".to_string(), |value| value.to_string())
}

pub fn text(value: &Option<String>) -> String {
    value.clone().unwrap_or_else(|| "none".to_string())
}

pub fn first_line(value: &str) -> String {
    value
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("empty")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_number_is_printed_as_itself() {
        assert_eq!(number(Some(12.5)), "12.5");
        assert_eq!(number(Some(0.0)), "0");
    }

    #[test]
    fn nothing_at_all_is_unset_rather_than_zero() {
        assert_eq!(number(None), "unset");
        assert_eq!(count(None), "unset");
        assert_eq!(text(&None), "none");
    }

    #[test]
    fn an_infinite_bound_is_named_because_it_becomes_i64_max_when_it_is_rounded() {
        assert_eq!(number(Some(f64::INFINITY)), "NOT FINITE: inf");
        assert_eq!(number(Some(f64::NEG_INFINITY)), "NOT FINITE: -inf");
    }

    #[test]
    fn a_nan_bound_is_named_because_it_is_never_equal_to_itself() {
        assert_eq!(number(Some(f64::NAN)), "NOT FINITE: NaN");
    }

    #[test]
    fn a_span_carries_both_ends_and_flags_either_one_that_is_not_finite() {
        assert_eq!(span(Some((1.0, 4.0))), "1 to 4");
        assert_eq!(span(Some((1.0, f64::INFINITY))), "1 to NOT FINITE: inf");
        assert_eq!(span(None), "none");
    }

    #[test]
    fn a_count_and_a_string_are_printed_as_given() {
        assert_eq!(count(Some(84)), "84");
        assert_eq!(text(&Some("currency".to_string())), "currency");
    }

    #[test]
    fn the_first_line_of_an_item_is_what_names_it_in_a_log() {
        assert_eq!(
            first_line("Item Class: Rings\nRarity: Rare\n"),
            "Item Class: Rings"
        );
    }

    #[test]
    fn leading_blank_lines_are_skipped_so_the_field_is_never_empty() {
        assert_eq!(
            first_line("\n\n  Orb of Augmentation\n"),
            "Orb of Augmentation"
        );
        assert_eq!(first_line(""), "empty");
        assert_eq!(first_line("   \n  "), "empty");
    }
}
