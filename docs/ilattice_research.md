# ilattice Crate Assessment

## Overview
- **Repository**: https://github.com/bonsairobo/ilattice-rs
- **Purpose**: Generic math utilities for integer lattices (regular 2D/3D grids) with tight integration with `glam` vector types.
- **Core Features**:
  - Trait-based abstractions for integer and real vector math (`IntegerVector`, extent utilities, morton encoding).
  - Re-exported `glam` types for convenience, providing `IVec2/3`, `UVec2/3`, and `Vec2/3` implementations.
  - Helpers for Morton (Z-curve) indexing to linearize 3D grids for cache-friendly storage.

## Strengths
- Minimal dependency footprint and actively published (0.4.0) with focused scope.
- Drop-in conversions for `glam` types simplify interoperability with Bevy transforms and chunk coordinates.
- Provides generic traits usable in custom data structures, enabling compile-time dimension configuration and SIMD-friendly math.

## Weaknesses / Risks
- Does not include storage or meshing primitives; only math helpers. Needs pairing with custom chunk storage.
- Limited documentation on integration patterns beyond lattice math; requires in-house design for chunk streaming.

## Applicability to Planetary CA MVP
- Useful for Morton indexing of chunk IDs and sub-voxel coordinates when building cache-friendly structures or GPU-friendly buffers.
- Can underpin custom clipmap/octree indexing without adopting large frameworks.
- Lightweight enough to include directly in MVP for coordinate math consistency alongside `big_space` high-precision transforms.

## Integration Notes
- Add dependency:
  ```toml
  [dependencies]
  ilattice = { version = "0.4", features = ["glam"] }
  ```
- Use Morton utilities for chunk atlas packing:
  ```rust
  use ilattice::morton::{MortonEncoder3D, morton_decode3d};

  let encoder = MortonEncoder3D::default();
  let morton_index = encoder.encode(IVec3::new(x, y, z));
  let coords = morton_decode3d(morton_index);
  ```
- Combine with `big_space` grid cells by storing `MortonKey` within chunk components for deterministic ordering during streaming.

## Recommendation
Adopt `ilattice` in the MVP to standardize integer lattice math, Morton ordering, and conversions with `glam`. Its focused feature set complements custom chunk storage without imposing heavy architecture constraints.
