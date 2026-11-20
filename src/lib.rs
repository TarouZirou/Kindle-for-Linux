//! Kindle for Linux — an ultra-light e-book / comic reader.
//!
//! `core` is the formally verified part (see `proof/`); everything else is
//! I/O and drawing.
pub mod app;
pub mod canvas;
pub mod core;
pub mod doc;
pub mod font;
pub mod library;
pub mod store;
pub mod worker;
