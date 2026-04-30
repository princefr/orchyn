// Modules
pub mod annotator;
pub mod observer;
pub mod types;

// Re-exports
pub use annotator::annotate_screenshot;
pub use observer::PixelObserver;
pub use types::*;
