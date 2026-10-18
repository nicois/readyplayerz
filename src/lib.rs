pub mod grid;
pub mod sim;

#[cfg(target_arch = "wasm32")]
mod render;
#[cfg(target_arch = "wasm32")]
mod web;
