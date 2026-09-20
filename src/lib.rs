//! Talking to MCHOSE mice. See PROTOCOL.md.

pub mod hidraw;
#[macro_use]
pub mod log;
pub mod preset;
pub mod proto;

pub mod i18n;

pub mod auto;

pub mod applications;

pub mod system;

pub mod desktop;

pub mod notifications;
