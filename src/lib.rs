pub mod config;
pub mod error;
pub mod load;
pub mod make_valid;
pub mod orient;
pub mod snap;

#[cfg(feature = "arrange")]
pub mod arrange;
pub mod noding;
#[cfg(feature = "structure")]
pub mod structure;

#[cfg(feature = "parallel")]
pub mod parallel;
#[cfg(feature = "simd")]
pub mod simd;

pub use config::{MakeValidConfig, PolyMethod};
pub use error::MakeValidError;
pub use make_valid::MakeValid;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;
