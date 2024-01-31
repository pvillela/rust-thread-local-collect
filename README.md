# thread-local-collect

This library supports the **_collection_** and **_aggregation_** of the values of a designated thread-local variable across threads.

An aggregation operation is applied to the values _collected_ from the the thread-local variables and the resulting accumulated value is made available to the library's caller.

This library contains multiple modules ([`joined`], [`simple_joined`], [`probed`], and [`channeled`]), with varying features and constraints, that support thread-local variable _collection_ and _aggregation_.

## Core concepts

The core concepts in this framework are the `Control` struct, the `Holder` struct, and the `HolderLocalKey` trait.
The library modules provide specific implementations of these core concepts.

`Holder` wraps a thread-local value, ensures that each such variable, when used, is linked with `Control`, and notifies `Control` when the `Holder` instance is dropped upon thread termination.

`Control` keeps track of the linked thread-local variables, contains an accumulation operation `op` and an accumulated value `acc`, and provides methods to access the accumulated value. The accumulated value is updated by applying `op` to each thread-local data value and `acc` when the thread-local value is collected. Depending on the specific module, thread-local values are collected when the `Holder` value is dropped and/or when collection is initiated by a method on the `Control` object.

Implementations of the `HolderLocalKey` trait for [`LocalKey`](https://doc.rust-lang.org/std/thread/struct.LocalKey.html)<`Holder`> provide methods to access the thread-local variables. (Recall that [`LocalKey`](https://doc.rust-lang.org/std/thread/struct.LocalKey.html) is the type underlying all thread-local variables.)

## Usage examples

See the different modules ([`joined`], [`simple_joined`], [`probed`], and [`channeled`]) for module-specific usage examples.
