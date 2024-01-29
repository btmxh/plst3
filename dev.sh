#!/bin/sh

trap 'kill $!' EXIT
CURRENT_PLAYLIST=1 RUST_LOG=info RUST_BACKTRACE=1 cargo run &
RUST_LOG=info RUST_BACKTRACE=1 plst3-bundler
wait
