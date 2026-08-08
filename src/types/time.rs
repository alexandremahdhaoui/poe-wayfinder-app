pub type Millis = u64;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_duration_in_milliseconds_survives_a_long_session() {
        let a_year: Millis = 365 * 24 * 60 * 60 * 1000;

        assert!(a_year < Millis::MAX);
    }
}
