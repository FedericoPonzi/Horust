#[macro_use]
extern crate crossbeam;
#[macro_use]
extern crate log;
#[macro_use]
extern crate maplit;

pub use crate::horust::{get_sample_service, Horust};

pub mod horust;
