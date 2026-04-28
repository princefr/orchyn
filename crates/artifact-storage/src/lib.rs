// --- Module declarations ---
pub mod traits;
pub mod error;
pub mod local;
pub mod mock;

#[cfg(feature = "s3")]
pub mod s3;

// --- Re-exports ---
pub use traits::*;
pub use error::*;
pub use local::*;
pub use mock::*;

#[cfg(feature = "s3")]
pub use s3::*;
