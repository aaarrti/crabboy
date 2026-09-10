set shell := ["zsh", "-cu"]

pull-data *args:
    dvc pull {{ args }}

run-tetoris *args:
    cargo run --profile dev -- --rom-path data/tetoris.gb --boot-rom-path data/boot.gb {{ args }}