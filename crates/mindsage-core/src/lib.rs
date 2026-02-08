//! MindSage Core — SDK entry point, device capabilities, configuration.

pub mod capabilities;
pub mod config;
pub mod error;

pub use capabilities::{CapabilityTier, DeviceCapabilities};
pub use config::{DataPaths, MindSageConfig};
pub use error::{Error, Result};
