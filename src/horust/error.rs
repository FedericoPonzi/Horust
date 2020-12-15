use std::fmt::{self, Display, Formatter};

pub type Result<T> = std::result::Result<T, HorustError>;

#[derive(Debug)]
pub enum ErrorKind {
    Io(std::io::Error),
    SerDe(toml::de::Error),
    NullError(std::ffi::NulError),
    Nix(nix::Error),
    ValidationError(Vec<ValidationError>),
    TemplatingError(templar::error::TemplarError),
}

#[derive(Debug)]
pub struct HorustError {
    kind: ErrorKind,
}

impl Display for HorustError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::result::Result<(), fmt::Error> {
        match &self.kind {
            ErrorKind::Io(error) => write!(f, "IoError: {}", error),
            ErrorKind::Nix(error) => write!(f, "NixError: {}", error),
            ErrorKind::NullError(error) => write!(f, "NullError: {}", error),
            ErrorKind::SerDe(error) => write!(f, "Deserialization error(Serde): {}", error),
            ErrorKind::ValidationError(error) => write!(f, "ValidationErrors: {:?}", error),
            ErrorKind::TemplatingError(error) => write!(f, "Template engine failed: {:?}", error),
        }
    }
}

impl std::error::Error for HorustError {}

impl From<ErrorKind> for HorustError {
    fn from(kind: ErrorKind) -> HorustError {
        HorustError { kind }
    }
}

impl From<toml::de::Error> for HorustError {
    fn from(err: toml::de::Error) -> Self {
        HorustError {
            kind: ErrorKind::SerDe(err),
        }
    }
}

impl From<std::io::Error> for HorustError {
    fn from(err: std::io::Error) -> Self {
        HorustError {
            kind: ErrorKind::Io(err),
        }
    }
}

impl From<nix::Error> for HorustError {
    fn from(err: nix::Error) -> Self {
        HorustError {
            kind: ErrorKind::Nix(err),
        }
    }
}

impl From<std::ffi::NulError> for HorustError {
    fn from(err: std::ffi::NulError) -> Self {
        HorustError {
            kind: ErrorKind::NullError(err),
        }
    }
}

impl From<Vec<ValidationError>> for HorustError {
    fn from(err: Vec<ValidationError>) -> Self {
        HorustError {
            kind: ErrorKind::ValidationError(err),
        }
    }
}

impl From<templar::error::TemplarError> for HorustError {
    fn from(err: templar::error::TemplarError) -> Self {
        HorustError {
            kind: ErrorKind::TemplatingError(err),
        }
    }
}

#[derive(Debug)]
pub struct ValidationError {
    kind: ValidationErrorKind,
    context: String,
}

#[derive(Debug)]
pub enum ValidationErrorKind {
    MissingDependency,
    CommandEmpty,
}

impl std::error::Error for ValidationError {}

impl ValidationError {
    pub fn new(context: &str, kind: ValidationErrorKind) -> Self {
        Self {
            context: context.to_string(),
            kind,
        }
    }
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::result::Result<(), fmt::Error> {
        write!(f, "{}", self.context)
    }
}
