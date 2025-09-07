we are doing DMG 


cargo build --release
cargo flamegraph --freq 99 -- --cartridge data/tetoris.gb --help



rgbasm -o green_bg.o green_bg.asm
rgblink -o green_bg.gb green_bg.o
rgbfix -v -p 0 green_bg.gb      # pad & fix header checksum (fine for most emulators)
