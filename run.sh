#!/bin/bash
RUSTFLAGS="-C target-cpu=native" time cargo build --release && \
	target/release/buttcoin $1 $2 $3
