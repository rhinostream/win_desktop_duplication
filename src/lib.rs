#![doc = include_str ! ("../README.md")]

use crate::errors::DDApiError;

pub mod devices;
pub mod outputs;
pub mod duplication;
mod utils;
pub mod errors;
pub mod texture;

pub use duplication::*;

pub type Result<T> = core::result::Result<T, DDApiError>;

