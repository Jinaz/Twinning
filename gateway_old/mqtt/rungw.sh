# build
cargo build --release

RUST_LOG=info cargo run --bin main -- \
  --host 127.0.0.1 --port 1883 \
  --subscribe-filter "fischertechnik/#" \
  --command-prefix "fischertechnik/commands" \
  --structured-event-prefix "fischertechnik/events"
