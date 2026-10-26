---
title: glTF custom vertex attributes are registered with their leading underscore
pull_requests: []
---

`GltfPlugin::add_custom_vertex_attribute` (and `GltfLoader::custom_vertex_attributes`) now match the attribute name exactly as it appears in the glTF file, as their documentation always said. glTF requires application-specific attribute names to start with an underscore, and that underscore was previously stripped before the lookup, so an attribute named `_FOO` in the file only matched a registration under `FOO`.

If you registered a custom attribute without its leading underscore to work around this, add it back:

```rust
// Before: matched the file's `_BARYCENTRIC`
GltfPlugin::default().add_custom_vertex_attribute("BARYCENTRIC", ATTRIBUTE_BARYCENTRIC)
// After
GltfPlugin::default().add_custom_vertex_attribute("_BARYCENTRIC", ATTRIBUTE_BARYCENTRIC)
```
