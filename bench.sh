#!/bin/bash

# Have to use environment instead of command line argument in cargo bench below because
# cargo bench doesn't parse arguments to target function properly when using `--`.
export TARGET_ARGS="$*"

cargo bench --all-features --bench benchmark
