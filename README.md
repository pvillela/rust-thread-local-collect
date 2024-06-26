# thread_local_collect

This library supports the **_collection_** and **_aggregation_** of thread-local data across threads. An aggregation operation is applied to the collected thread-local values and the resulting accumulated value is made available to the library's caller. This library contains multiple modules, with varying features and constraints.

See the crate [documentation](https://docs.rs/thread_local_collect/latest/thread_local_collect/) for a comprehensive overview and usage examples. The source [repo](https://github.com/pvillela/rust-thread-local-collect/tree/main) also contains benchmarks and additional examples.

## Rust version requirements

This version of this library can be compiled with rustc 1.79.0 or higher. It may work with earlier rustc versions but that is not guaranteed.

## License

This library is distributed under the terms of the MIT license, with copyright retained by the author.

See [LICENSE](https://github.com/pvillela/rust-thread-local-collect/tree/main/LICENSE) for details.
