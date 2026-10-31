//! Tablet input to virtual mouse / Xbox 360 gamepad bridge.
//!
//! The driver reads raw evdev events from a Wacom pen (and its companion
//! ExpressKey pad), translates them according to a preset, and emits events
//! through `/dev/uinput`. Keyboard output is intentionally not implemented:
//! key bindings belong to the game, not to this adapter.

pub mod action;
pub mod cli;
pub mod completion;
pub mod config;
pub mod device;
pub mod engine;
pub mod mapping;
pub mod mouse;
pub mod output;
pub mod presets;
