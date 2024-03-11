#!/bin/bash

cargo makedocs -e log -e thread_local
cargo doc -p thread-local-collect --no-deps --all-features
