//! All modules for the pipeline, which is the main data processing and reporting system of the collector.

pub mod dataset;
pub mod delta;
mod idle;
pub mod metrics;
pub mod reporter;
pub mod rows;
pub mod segments;
