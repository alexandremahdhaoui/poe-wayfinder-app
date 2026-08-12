use std::sync::OnceLock;
use std::time::Instant;

static START: OnceLock<Instant> = OnceLock::new();

pub fn start() {
    let _ = START.get_or_init(Instant::now);
}

pub fn millis() -> i64 {
    START.get_or_init(Instant::now).elapsed().as_millis() as i64
}

pub fn field() -> (&'static str, crate::logging::Value) {
    ("elapsed_ms", crate::logging::Value::Int(millis()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clock_starts_at_the_first_call_and_never_goes_backwards() {
        start();

        let first = millis();
        let second = millis();

        assert!(first >= 0);
        assert!(second >= first, "{second} < {first}");
    }

    #[test]
    fn the_field_is_named_so_a_log_reader_can_grep_it() {
        let (name, value) = field();

        assert_eq!(name, "elapsed_ms");
        assert!(matches!(value, crate::logging::Value::Int(_)));
    }

    #[test]
    fn calling_start_twice_does_not_reset_the_clock() {
        start();

        let before = millis();

        start();

        assert!(
            millis() >= before,
            "a second start must not rewind the clock"
        );
    }
}
