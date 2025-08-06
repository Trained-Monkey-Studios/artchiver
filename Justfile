
plugin:
    pushd plugins/artx-demo && cargo build --release --target wasm32-unknown-unknown && popd
    pushd plugins/artx-met && cargo build --release --target wasm32-unknown-unknown && popd
    pushd plugins/artx-nga && cargo build --release --target wasm32-unknown-unknown && popd

fmt:
    cargo fmt --all
