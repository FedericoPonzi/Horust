use std::ffi::NulError;
use std::fmt::{self, Display, Formatter};

pub type Result<T> = std::result::Result<T, HorustError>;

#[derive(Debug)]
pub enum ErrorKind {
    Io(std::io::Error),
    SerDe(toml::de::Error),
    NullError(std::ffi::NulError),
    Nix(nix::Error),
}

#[derive(Debug)]
pub struct HorustError {
    kind: ErrorKind,
}
impl Display for HorustError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::result::Result<(), fmt::Error> {
        write!(f, "HorustError")
    }
}
impl HorustError {
    /// Return the kind of this error.
    pub fn kind(&self) -> &ErrorKind {
        &self.kind
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
    fn from(err: NulError) -> Self {
        HorustError {
            kind: ErrorKind::NullError(err),
        }
    }
}
