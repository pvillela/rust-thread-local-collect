#!/bin/bash

cargo nextest run --lib --bins --examples --tests --all-features
cargo test --doc --all-features
