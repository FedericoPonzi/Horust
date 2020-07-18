// Remove once https://github.com/FedericoPonzi/Horust/issues/42 is fixed
mod dummy;

#[macro_use]
extern crate log;

#[macro_use]
extern crate maplit;

#[macro_use]
extern crate crossbeam;

pub mod horust;
pub use crate::horust::{get_sample_service, Horust, HorustError};
