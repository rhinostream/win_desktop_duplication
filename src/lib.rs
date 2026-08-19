#![doc = include_str ! ("../README.md")]

use crate::errors::DDApiError;

pub mod devices;
pub mod outputs;
pub mod duplication;
mod utils;
pub mod errors;
pub mod texture;
pub mod tex_reader;

mod shader;
mod cursor_renderer;

pub mod cursor;




pub use duplication::*;
pub use utils::{co_init,set_process_dpi_awareness};

pub type Result<T> = core::result::Result<T, DDApiError>;

