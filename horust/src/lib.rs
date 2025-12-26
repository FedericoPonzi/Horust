#[macro_use]
extern crate crossbeam;
#[macro_use]
extern crate log;
#[macro_use]
extern crate maplit;

pub use crate::horust::{Horust, get_sample_service};

pub mod horust;
