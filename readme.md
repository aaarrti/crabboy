we are doing DMG 


cargo build --release
timeout 60s /usr/lib/linux-tools-6.8.0-79/perf record -g ./target/release/crabboy --cartridge data/tetoris.gb
/usr/lib/linux-tools-6.8.0-79/perf report


cargo flamegraph -- --cartridge data/tetoris.gb