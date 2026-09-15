# Flux derive macros

`Reactive` generates an implementation against the consuming crate's Flux
prelude and registers the reflected type. Runtime behavior belongs to Flux and
Flux Core; this proc-macro crate needs only token parsing and crate-name lookup.
Keeping runtime dependencies out of the macro's host graph reduces work for
native builds and for cross-compilation to ESP or WebAssembly.

Dependency renaming is supported through `proc-macro-crate`. The input is parsed
directly as a Syn `DeriveInput` so compiler diagnostics retain source spans.
The generated registration and implementation remain unchanged.

## Checked binding paths and pipelines

The function-like macros are re-exported by `flux::prelude::*`. They generate
normal Rust field accesses in closures that are type-checked but never called:
no component instance, query, or record is created to validate a path.

```rust,ignore
path!(entity, Model.number)                  // Result<TypedBindingPath<i32>> (if number: i32)
component_path!(Control.is_visible)                  // Result<ComponentBindingPath>
property_path!(Device.wifi_configs[0].ssid)           // String, root omitted
path!(entity, View.device_id -> Device.name)  // Id entity jump
path!(entity, ReactiveView.value as WifiConfig.ssid) // dynamic shape
```

Use `?` in a field chain to type-check access through an Option:
`property_path!(Model.settings?.volume)`. The generated path omits `?` because
Flux already unwraps intermediate Options. A final Option needs no `?`.
Literal list indices and tuple fields are supported; runtime indices, methods,
and enum variant selection are intentionally not path syntax.

`-> Component` appends a component name and checks that the preceding typed
value is `Id` or `Option<Id>`. For a dynamic value containing an Id, spell that
shape explicitly: `ReactiveView.value as Id -> Device.wifi_configs`.
`as Type` checks subsequent fields against Type but appends no type name. It
neither performs a cast nor proves the actual runtime dynamic shape.

The macro preserves the final field type in `TypedBindingPath<T>`.
`process` / `process_system` preserve their output in `BindingExpr<T>`, and
`commands.set_property(path!(entity, Model.number), 42)` checks the
value type at compile time. Use `.erase()` for explicitly dynamic APIs.

Root and jump component names come from Bevy TypePath, not the textual spelling
of an import or type alias. Ordinary fields obey Rust visibility and type rules;
a typo is a compilation error. Reflection registration, record availability,
list bounds, and dynamic shape compatibility are still runtime requirements.

```rust,ignore
let filtered = binding_node!(graph;
    source(app_entity, AppView.devices)
    => filter::<Id>(|id| *id != Id::nil())
)?;
let label = binding_node!(graph;
    node(selected_agent)
    => jump(User.name)
    => map_value::<String, String>(|name| Ok(format!("Hello, {name}")))
)?;
```

Pipeline stages: `filter::<T>(predicate)`, `map_value::<T, U>(function)`,
`path(Type.field)` for a value projection, `jump(Component.field)` for an Id
lookup, `system(name, function)` for ECS-aware logic, and
`then(function)` for custom graph composition. A then function receives
`(&mut BindingGraph, BindingNode)` and returns `BindingResult<BindingNode>`.
Start with `node(existing)` to reuse a union or other multi-input result;
`system(name, function)` can also start a pipeline with no explicit inputs.
Each input expression is evaluated once. A pipeline returns a Result and stops
at the first error; nodes already added to the graph remain on construction error.
Use `?` and discard that unfinished graph on failure.

The parser uses Syn's `full` feature for entity expressions and closures. It
adds no Bevy/Flux runtime dependency to the proc-macro host graph. Crate lookup
uses `proc-macro-crate`, including renamed Flux dependencies.

See [Flux's binding guide](../flux/docs/binding-graphs.md) for builder integration,
compatibility scheduling, and examples based on Empathic's Devices views.

## Bevy HashMap literal

`bevy_hash_map!` has the familiar `map_macro` key/value syntax, but constructs
the result through `FromIterator` so it works with Bevy's platform-selected
`HashMap`:

```rust,ignore
use bevy_platform::collections::HashMap;
use flux::prelude::bevy_hash_map;

let values: HashMap<&str, i32> = bevy_hash_map! {
    "one" => 1,
    "two" => 2,
};
```
