//! Web 面板 HTTP 服务（v1 master 内置）。
//!
//! 与 v0 `src/panel/*` 完全独立：v0 panel 仍由 `relay-rs daemon` 提供（端口 9090
//! 默认），v1 panel 由 `relay-master daemon` 提供（默认 0.0.0.0:9090，由
//! `RELAY_PANEL_LISTEN` 控制）。两者**不应**部署在同一端口。

mod assets;
mod auth;
mod discourse;
mod server;
pub mod settings;

pub use server::run;
