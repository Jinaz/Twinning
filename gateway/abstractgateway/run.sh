# Build
cargo build

# Set configuration
cargo run --bin main -- set-config 127.0.0.1 user pass '{"qos":1,"retain":false}'

# Add a property
cargo run --bin main -- set-prop motor/speed float 1200

# Add a method with variables
cargo run --bin main -- set-method calibrate '{"target":42,"mode":"fast"}'

# Execute the method (test prints)
cargo run --bin main -- exec calibrate

# Show current state (note: in this simple version state is not persisted across runs)
cargo run --bin main -- show
