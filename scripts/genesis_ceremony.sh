#!/bin/bash
set -e

# Build the genesis tool
echo "Building genesis-tool..."
cargo build --release --bin genesis-tool

# Create output directory
mkdir -p genesis_output

# Run the tool
echo "Generating keys and genesis..."
./target/release/genesis-tool --output genesis_output/genesis.json --validators 4

# Move keys to output
mv validator_*.secret genesis_output/

# Verification Checks
echo "Verifying outputs..."
if [ ! -f "genesis_output/genesis.json" ]; then
    echo "Error: genesis.json not found!"
    exit 1
fi

COUNT=$(ls genesis_output/validator_*.secret | wc -l)
if [ "$COUNT" -ne 4 ]; then
    echo "Error: Expected 4 validator keys, found $COUNT"
    exit 1
fi

echo "Genesis ceremony complete and verified. Files in genesis_output/"
