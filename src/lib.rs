#[macro_use]
extern crate log;

#[macro_use]
extern crate maplit;

#[macro_use]
extern crate crossbeam;

pub mod horust;
pub use crate::horust::{get_sample_service, Horust};
