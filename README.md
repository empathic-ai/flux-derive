# Flux derive macros

`Reactive` generates an implementation against the consuming crate's Flux
prelude and registers the reflected type. Runtime behavior belongs to Flux and
Flux Core; this proc-macro crate needs only token parsing and crate-name lookup.
Keeping runtime dependencies out of the macro's host graph reduces work for
native builds and for cross-compilation to ESP or WebAssembly.

Dependency renaming is supported through `proc-macro-crate`. The input is parsed
directly as a Syn `DeriveInput` so compiler diagnostics retain source spans.
The generated registration and implementation remain unchanged.
