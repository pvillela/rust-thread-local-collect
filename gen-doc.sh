#!/bin/bash

cargo makedocs -e log -e thread_local
cargo doc -p thread_local_collect --no-deps --all-features
