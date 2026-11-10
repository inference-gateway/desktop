use std::fmt;

/// Why a command failed. It reaches the frontend as its message, the same
/// string commands returned before they had a type, so nothing there changes.
#[derive(Debug)]
pub(crate) enum Error {
    Io(std::io::Error),
    Json(serde_json::Error),
    /// A background task that panicked or was cancelled.
    Task(tokio::task::JoinError),
    /// A failure already worded for the person, as the modules that still
    /// return `String` word theirs.
    Message(String),
}

pub(crate) type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(f),
            Self::Json(error) => error.fmt(f),
            Self::Task(error) => error.fmt(f),
            Self::Message(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::Task(error) => Some(error),
            Self::Message(_) => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for Error {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl From<tokio::task::JoinError> for Error {
    fn from(error: tokio::task::JoinError) -> Self {
        Self::Task(error)
    }
}

impl From<String> for Error {
    fn from(message: String) -> Self {
        Self::Message(message)
    }
}

impl From<&str> for Error {
    fn from(message: &str) -> Self {
        Self::Message(message.to_owned())
    }
}

impl serde::Serialize for Error {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

/// Runs blocking work off the async workers and the main thread. A task that
/// panics is an error like any other.
pub(crate) async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T> {
    tokio::task::spawn_blocking(work).await?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_error_reaches_the_frontend_as_its_message() {
        let worded = Error::from("invalid repository: x");
        assert_eq!(
            serde_json::to_string(&worded).unwrap(),
            r#""invalid repository: x""#
        );

        let parse = serde_json::from_str::<u8>("nope").unwrap_err();
        let message = parse.to_string();
        assert_eq!(
            serde_json::to_value(Error::from(parse)).unwrap(),
            serde_json::Value::String(message)
        );
    }

    #[test]
    fn a_wrapped_error_keeps_its_cause() {
        use std::error::Error as _;
        let missing = std::io::Error::new(std::io::ErrorKind::NotFound, "gone");
        assert!(Error::from(missing).source().is_some());
        assert!(Error::from("worded").source().is_none());
    }
}
