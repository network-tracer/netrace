//! Generates the Swift bindings, C header and modulemap for the app.
//! Run through `scripts/build-core.sh`, not directly.

fn main() {
    uniffi::uniffi_bindgen_swift()
}
