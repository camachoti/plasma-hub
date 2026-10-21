use thiserror::Error;

/// Domain error type shared by the Plasma Hub crates.
///
/// The Tauri/Dioxus boundary converts this into a message (or a serialised
/// error) while internal code can match on the variant.
#[derive(Debug, Error)]
pub enum Error {
    #[error("{0}")]
    Message(String),

    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("json: {0}")]
    Json(#[from] serde_json::Error),

    #[error("download cancelado pelo usuário")]
    Cancelled,

    #[error("{context}: {source}")]
    Context {
        context: String,
        #[source]
        source: Box<Error>,
    },
}

impl Error {
    pub fn msg(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }

    pub fn context(message: impl Into<String>, source: impl Into<Error>) -> Self {
        Self::Context {
            context: message.into(),
            source: Box::new(source.into()),
        }
    }

    pub fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled)
    }
}

impl From<String> for Error {
    fn from(value: String) -> Self {
        Self::Message(value)
    }
}

impl From<&str> for Error {
    fn from(value: &str) -> Self {
        Self::Message(value.to_string())
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_strings_and_io_errors() {
        let string_error: Error = "boom".into();
        assert_eq!(string_error.to_string(), "boom");

        let io_error: Error = std::io::Error::new(std::io::ErrorKind::NotFound, "nope").into();
        assert!(io_error.to_string().starts_with("io:"));
    }

    #[test]
    fn detects_cancellation() {
        assert!(Error::Cancelled.is_cancelled());
        assert!(!Error::msg("other").is_cancelled());
    }
}
