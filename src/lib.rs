#[macro_use]
extern crate log;

#[macro_use]
extern crate maplit;

pub mod horust;
pub use crate::horust::{get_sample_service, Horust, HorustError};
