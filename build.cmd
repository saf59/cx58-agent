set RUST_BACKTRACE=full
set path=%path%;C:\msys64\ 
@rem cmake
@cargo build --release
cargo clippy