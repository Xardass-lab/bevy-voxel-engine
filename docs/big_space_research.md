# `big_space` Coordinate System Research

## Highlights
- Big Space advertises "huge worlds, high performance, no dependencies" and reuses `Transform`/`GlobalTransform` so existing Bevy systems continue to function ([README](https://github.com/aevyrie/big_space)).
- Supports precision from proton to observable-universe scale by chaining integer grids (`i8` up to `i128`) and floating origins ([README highlights](https://github.com/aevyrie/big_space/blob/main/README.md#highlights)).
- Provides spatial hashing (`GridHashMap`) and partitioning helpers to accelerate neighbor lookup for large entity sets ([docs.rs Quick Reference](https://docs.rs/big_space/latest/big_space/)).

## Core Concepts
- **BigSpace**: root component that anchors a high-precision hierarchy and holds grid parameters for descendants.
- **FloatingOrigin**: entity whose transform defines the local render origin, minimizing floating point error for 32-bit GPU transforms ([docs.rs Floating Origin](https://docs.rs/big_space/latest/big_space/#floating-origin)).
- **Grid / GridCell**: define integer cell size and indices for nested grids, letting you partition the world into coarse-to-fine spatial buckets without coordinate drift ([docs.rs Integer Grid](https://docs.rs/big_space/latest/big_space/#integer-grid)).

## Integration Sketch
```rust
use big_space::prelude::*;
use bevy::prelude::*;

fn setup(mut commands: Commands) {
    commands.spawn((BigSpace::default(), SpatialBundle::default()));
    commands.spawn((
        FloatingOrigin,
        Camera3dBundle::default(),
        Grid::new(GridPrecision::Int64, 1024.0),
    ));
    commands.spawn((
        GridCell::new(IVec3::ZERO),
        Transform::from_translation(Vec3::new(0.0, 0.0, 0.0)),
        GlobalTransform::default(),
    ));
}
```
The plugin rewrites `Transform` propagation so entities stay centered around the origin while preserving absolute `GridCell` indices for simulation logic.

## Benefits for Planetary CA
- Integer grids let you address voxels across planetary scales without precision loss; CA updates can operate on `(grid_cell, local_pos)` pairs, while rendering works in f32 space relative to the floating origin.
- Spatial hash (`GridHash`) allows rapid neighbor queries across chunk boundaries, useful for diffusing CA states and synchronizing fracture fronts.
- Compatible with `oktree` or chunked voxel storage: treat each chunk as a `GridCell` child and maintain absolute indexing for streaming.

## Considerations
- You must manage recentering frequency: `FloatingOrigin` systems recenter when the tracked entity leaves a cell; ensure CA job scheduling tolerates the momentary `Transform` update.
- Physics/gravity require custom integration with Big Space coordinates; align gravitational acceleration toward the planetary center expressed in high-precision grid space, then convert to local `Transform` for rendering.
- Network replication or save files should serialize `GridCell` + local transform rather than raw floats to avoid drift.
