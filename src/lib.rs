//! samba-ui：零依赖的极简 Samba smb.conf 可视化管理页面。

pub mod app;
pub mod args;
pub mod conf;
pub mod html;
pub mod http;
pub mod pages;
pub mod routes;
pub mod store;
pub mod users;
pub mod util;

pub use app::run;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
