#![forbid(unsafe_code)]

pub mod cross_section;
pub mod error;
pub mod evaluate;
pub mod factor;
pub mod factors;
pub mod grpc;
pub mod storage;

pub use error::{FactorError, Result};
pub use factor::{
    default_registry, DataDependency, Factor, FactorCategory, FactorDirection, FactorMeta,
    FactorRegistry,
};
