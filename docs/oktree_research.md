# Oaktree (`oktree`) Research

## Crate Overview
- **Crate**: [`oktree`](https://crates.io/crates/oktree) v0.4.1
- **Purpose**: High-performance, pointer-free sparse voxel octree with optional Bevy integration feature flag.
- **Key design points**:
  - Avoids smart pointers to maximize cache locality and performance, instead using contiguous buffers and indices for nodes ([docs.rs reference](https://docs.rs/oktree/0.4.1/oktree/)).
  - Provides Bevy feature flag (`features = ["bevy"]`) to expose intersection methods and component wrappers for engine integration ([docs.rs feature list](https://docs.rs/oktree/0.4.1/oktree/#features)).

## Benchmark Summary
The published benchmark for `oktree` uses a `4096³` volume and shows the following timings on release builds (`cargo bench --all-features`) [source](https://docs.rs/oktree/0.4.1/oktree/#benchmark):

| Operation | Quantity | Time |
|-----------|----------|------|
| Insertion | 65,536 cells | 21 ms |
| Removal | 65,536 cells | 1.5 ms |
| Point lookup | 65,536 searches | 12 ms |
| Ray intersection | 4,096 rays vs 65,536 cells | 37 ms |
| Sphere intersection | 4,096 spheres vs 65,536 cells | 8 ms |
| Box intersection | 4,096 boxes vs 65,536 cells | 7 ms |

These numbers imply ~3.1M insertions per second and ~2.8M ray tests per second on the reference hardware. For CA workloads the branch factor (8) suits sparse activation patterns (e.g., cellular surfaces or shells) rather than densely active solids.

## Integration Notes
- Initial tree construction uses `Octree::from_aabb_with_capacity(aabb, capacity)` where capacity is the maximum leaf load before subdivision; tune to control tree depth vs. branching overhead ([docs.rs usage](https://docs.rs/oktree/0.4.1/oktree/#example)).
- Ray/sphere/box intersection helpers depend on the Bevy feature flag; enabling it adds `bevy` crate as a dependency and exports bundles for rendering debug draws.
- The crate exposes `Octree::iter()` and neighborhood traversal utilities to stream active cells into GPU-friendly buffers for chunk meshing.

## Suitability for Planetary CA
- Works best for sparse volumes (e.g., thin atmosphere layers or crust shells). Dense planetary interiors will cause high branching depth and degrade cache performance. Combining octrees with chunked voxel bricks (e.g., `32³` dense tiles) reduces pointer chasing by limiting tree depth.
- Node capacity is stored as `Unsigned` generics (`u8`..`u128`), enabling very deep hierarchies for huge worlds, but memory usage grows exponentially if the planet interior is densely populated.
- In scenarios with frequent large-scale edits (splitting planet), consider hybrid approach: maintain coarse-grained `oktree` for active fracture front and offload interior to chunked arrays; leverage `oktree` for collision/visibility queries.

## Alternative Structures
- **Sparse grids with paging** (e.g., hash-map keyed chunks) offer predictable memory/performance for dense fill factors and are easier to stream to GPU.
- **Voxel DAGs / Sparse Voxel Octrees (SVO)** are heavier to update but compress static regions better—useful for far-field read-only LOD once the CA stabilizes.
- **Dual contouring on chunked terrain** is friendlier to incremental meshing than per-voxel ray queries; combine with compute shaders for fracture updates when splitting the planet.

In summary, `oktree` is suitable for sparse, high-frequency query workloads (collisions, neighbor search) but should be combined with chunked dense storage for planetary cellular automata.
