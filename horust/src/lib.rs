#[macro_use]
extern crate crossbeam_channel;
#[macro_use]
extern crate log;

pub use crate::horust::{Horust, get_sample_service};

pub mod horust;
