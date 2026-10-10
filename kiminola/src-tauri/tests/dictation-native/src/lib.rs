#![cfg(windows)]
#![allow(dead_code)]
extern crate self as tauri_plugin_global_shortcut;
pub use global_hotkey::hotkey::{Code, HotKey as Shortcut, Modifiers};

#[path = "../../../src/dictation_native.rs"]
mod dictation_native;
