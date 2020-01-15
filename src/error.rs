use std::fmt::{Display, Formatter};
use std::io::Error;
use std::{fmt, io};

pub type Result<T> = std::result::Result<T, HorustError>;

#[derive(Debug)]
pub enum ErrorKind {
    Io(std::io::Error),
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
impl From<std::io::Error> for HorustError {
    fn from(err: Error) -> Self {
        HorustError {
            kind: ErrorKind::Io(err),
        }
    }
}
