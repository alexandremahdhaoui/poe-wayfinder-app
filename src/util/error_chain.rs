use std::error::Error;

const MAX_CAUSES: usize = 16;

pub fn render(error: &dyn Error) -> String {
    let mut parts = vec![error.to_string()];
    let mut source = error.source();
    let mut walked = 0;

    while let Some(cause) = source {
        if walked >= MAX_CAUSES {
            parts.push("...".to_string());

            break;
        }

        walked += 1;

        let text = cause.to_string();

        if parts.last().map(String::as_str) != Some(text.as_str()) {
            parts.push(text);
        }

        source = cause.source();
    }

    parts.join(": ")
}

#[cfg(test)]
mod tests {
    use super::*;

    use thiserror::Error;

    #[derive(Debug, Error)]
    #[error("the deepest thing")]
    struct Leaf;

    #[derive(Debug, Error)]
    #[error("the middle thing")]
    struct Middle(#[source] Leaf);

    #[derive(Debug, Error)]
    #[error("the outer thing")]
    struct Outer(#[source] Middle);

    #[test]
    fn a_lone_error_is_just_its_message() {
        assert_eq!(render(&Leaf), "the deepest thing");
    }

    #[test]
    fn every_cause_is_rendered() {
        assert_eq!(
            render(&Outer(Middle(Leaf))),
            "the outer thing: the middle thing: the deepest thing"
        );
    }

    #[test]
    fn a_wrapper_that_repeats_its_cause_is_not_printed_twice() {
        #[derive(Debug, Error)]
        #[error("the deepest thing")]
        struct Echo(#[source] Leaf);

        assert_eq!(render(&Echo(Leaf)), "the deepest thing");
    }

    #[test]
    fn a_long_chain_is_cut_rather_than_followed_forever() {
        #[derive(Debug)]
        struct Loop;

        impl std::fmt::Display for Loop {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "round and round")
            }
        }

        impl Error for Loop {
            fn source(&self) -> Option<&(dyn Error + 'static)> {
                Some(&Loop)
            }
        }

        let got = render(&Loop);

        assert!(got.ends_with("..."), "{got}");
        assert!(got.len() < 400, "{got}");
    }
}
