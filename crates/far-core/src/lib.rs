//! Core of Far Ray Tracer: a Rust port of the engine of a 2008 final-year project.
//!
//! Classic mode reproduces the original program pixel by pixel, defects included.
//! Those defects stay on purpose when they change the image, and each one is
//! commented where it happens. Enhanced mode fixes them.

pub mod bump;
pub mod environment;
pub mod ggx;
pub mod surface;
pub mod camera;
pub mod caustics;
pub mod denoise;
pub mod enhanced;
pub mod engine;
pub mod kdtree;
pub mod mesh3ds;
pub mod preview;
pub mod scene;
mod scene_file;
pub mod solid;
pub mod texture;
pub mod twister;
pub mod vector;

pub use engine::{render, tile_size, Image, Mode, Options, Progress};
pub use enhanced::Bounces;
pub use kdtree::KdTree;
pub use scene::Scene;

pub use camera::{Camera, Pose};
