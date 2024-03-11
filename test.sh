#!/bin/bash

cargo nextest run --all-targets --all-features
cargo test --doc --all-features
