//! Backend command groups. The request transport and shared flag types come
//! from the engine gateway; argument parsing, help and output stay here.
pub(crate) use allowit_sdk_engine::gateway::*;

pub(crate) mod args;
pub(crate) mod control;
pub(crate) mod help;
pub(crate) mod http;
pub(crate) mod operation;
pub(crate) mod target;
