//! Shared launcher models and platform front ends. Android does not link the desktop UI or updater.

#[cfg(feature = "desktop")]
mod app;
pub mod catalog;
pub mod display;
#[cfg(feature = "desktop")]
mod dlss;
#[cfg(feature = "desktop")]
mod fomod;
#[cfg(feature = "desktop")]
mod fonts;
pub mod launch;
#[cfg(feature = "desktop")]
mod modlist;
#[cfg(any(feature = "mobile", test))]
pub mod mobile;
pub mod news;
#[cfg(feature = "desktop")]
mod reports;
pub mod start;
mod theme;
#[cfg(feature = "desktop")]
mod update;

#[cfg(feature = "desktop")]
pub use app::App;
