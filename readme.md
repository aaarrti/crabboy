we are doing DMG 


cargo build --release
timeout 10s /usr/lib/linux-tools-6.8.0-79/perf record -g ./target/release/crabboy data/tetoris.gb
/usr/lib/linux-tools-6.8.0-79/perf report
cargo flamegraph