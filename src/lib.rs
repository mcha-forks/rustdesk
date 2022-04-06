pub mod platform;
pub use platform::{get_cursor, get_cursor_data, get_cursor_pos, start_os_service};

mod server;
pub use self::server::*;

mod client;

mod rendezvous_mediator;
pub use self::rendezvous_mediator::*;

pub mod common;
pub mod ipc;
pub mod ui;

mod version;
pub use version::*;

use common::*;

#[cfg(feature = "cli")]
pub mod cli;

mod port_forward;
mod lang;
