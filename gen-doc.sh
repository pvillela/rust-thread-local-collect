#!/bin/bash

cargo makedocs -e log -e thread_local -e thread_map -e thiserror
cargo doc -p thread_local_collect --no-deps --all-features
