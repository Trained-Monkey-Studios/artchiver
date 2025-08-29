
plugin:
    pushd plugins/artx-demo && cargo build --release --target wasm32-unknown-unknown && popd
    pushd plugins/artx-met && cargo build --release --target wasm32-unknown-unknown && popd
    pushd plugins/artx-nga && cargo build --release --target wasm32-unknown-unknown && popd
    pushd plugins/artx-podcast && cargo build --release --target wasm32-unknown-unknown && popd

clippy:
    pushd plugins/artchiver_sdk && cargo clippy --target wasm32-unknown-unknown && popd
    pushd plugins/artx-demo && cargo clippy --target wasm32-unknown-unknown && popd
    pushd plugins/artx-met && cargo clippy --target wasm32-unknown-unknown && popd
    pushd plugins/artx-nga && cargo clippy --target wasm32-unknown-unknown && popd
    pushd plugins/artx-podcast && cargo clippy --target wasm32-unknown-unknown && popd
    cargo clippy --all --all-targets

fmt:
    pushd plugins/artchiver_sdk && cargo fmt && popd
    pushd plugins/artx-demo && cargo fmt && popd
    pushd plugins/artx-met && cargo fmt && popd
    pushd plugins/artx-nga && cargo fmt && popd
    pushd plugins/artx-podcast && cargo fmt && popd
    cargo fmt --all
