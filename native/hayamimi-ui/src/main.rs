//! The entry of the page bundle. It is built for `wasm32-unknown-unknown` with
//! the `web` feature and loaded by `web/index.html`.

fn main() {
    dioxus::launch(hayamimi_ui::App);
}
