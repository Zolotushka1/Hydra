//! Hydra panel frontend, Leptos CSR.
//!
//! One vertical slice: log in, fetch the bootstrap document, render it. Not a
//! type layer and not a client library — those get built out screen by screen,
//! behind something that already works end to end.
//!
//! The wire types come from `panel-domain`, the same crate the server
//! serialises from. Nothing here restates a field name or a shape, so a change
//! on the server is a compile error here rather than a runtime surprise.

mod api;
mod screens;

use crate::screens::App;

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(App);
}
