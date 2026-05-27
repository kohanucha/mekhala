# `put_multiple_raw` vs `put_multiple` — DO Hibernation & JS Map Pitfall

## Problem

`put_multiple` with a `HashMap<String, Value>` silently stores **zero entries** in Durable Object storage, returning `Ok(())` with no error. All subscription state vanishes after DO hibernation wake, producing NWC reply timeouts.

## Root Cause

Two things combine to produce this bug:

### 1. `serde_wasm_bindgen` default `Serializer` creates JS `Map`

`serde_wasm_bindgen`'s default `Serializer::new()` has `serialize_maps_as_objects: false`. When Rust's `HashMap` is serialized via `serde_wasm_bindgen::to_value(&hashmap)`, it produces a JS `Map`, not a plain JS `Object`.

### 2. DO runtime iterates via `GetOwnPropertyNames()`

`workerd`'s implementation of `put_multiple` iterates the argument using the C++ equivalent of `Object.getOwnPropertyNames()`. This returns `[]` on a JS `Map` — Map entries are stored in the `[[MapData]]` internal slot, not as own properties. Result: zero entries stored, `Ok(())` returned.

## Why Individual `put(key, value)` Works

`put(key_string, value)` passes the key as a separate string argument (not iterated), and the value is a plain JsValue. The key is used directly as a storage key regardless of how the value was constructed.

## Fix

Use `put_multiple_raw` with a manually constructed `js_sys::Object`:

```rust
let obj = Object::new();
for (k, v) in &entries {
    let key = JsValue::from(k.as_str());
    if let Ok(val) = v.serialize(&Serializer::json_compatible()) {
        let _ = Reflect::set(&obj, &key, &val);
    }
}
self.storage.put_multiple_raw(obj).await.map_err(|e| e.to_string())?;
```

Key points:
- `js_sys::Object::new()` creates a plain `{}` with real enumerable properties
- `Reflect::set` sets each property so `GetOwnPropertyNames()` finds it
- `Serializer::json_compatible()` still produces the correct JsValue for each value
- `put_multiple_raw` is a single round-trip (efficient)

## Detection

After waking from hibernation, `WalletRegistry::load()` logs:
```
restored conn={} with {} subscriptions
```

If subscriptions are consistently 0 after wake, this bug is active.

## Test Coverage

This is a WASM/`workerd`-specific bug that does not reproduce in pure Rust unit tests (`MockStorage` uses `HashMap` internally, no JS serialization). Only integration tests with the actual Worker runtime can catch it.
