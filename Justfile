
plugin:
    pushd plugins/the_met && cargo build --target wasm32-unknown-unknown && popd
    pushd plugins/nga && cargo build --target wasm32-unknown-unknown && popd

fmt:
    cargo fmt --all
