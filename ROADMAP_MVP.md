# Planetary Cellular Automata Roadmap (Room → Planet)

This roadmap outlines concrete steps and code glue required to scale the current Bevy voxel engine from room-sized simulations to a destructible planet with gravity pointed toward its core and real-time slicing.

## Stage 0 — Room-Scale Prototype
1. **Data Model**
   - Store CA state in a dense 3D array per chunk (`Vec<[State; CHUNK_VOLUME]>`). Use existing Bevy ECS chunk entities with `Component` resources for voxels.
   - Add a `SimulationSpeed` resource to scale `Time::delta_seconds()` when the CA cost exceeds budget.
2. **Simulation Loop**
   - Implement systems scheduled in a custom `UpdateSet::Simulation` before rendering to maintain determinism.
   - Example system:
     ```rust
     fn step_chunk(
         mut chunks: Query<(&ChunkCells, &mut ChunkCellsNext)>,
         sim_speed: Res<SimulationSpeed>,
         pool: Res<ComputeTaskPool>,
     ) {
         pool.scope(|scope| {
             for (cells, mut next) in chunks.iter_mut() {
                 scope.spawn(async move {
                     next.copy_from(cells);
                     run_room_rules(&mut next.0, sim_speed.factor);
                 });
             }
         });
     }
     ```
     Uses `bevy_tasks::ComputeTaskPool` for parallel iteration ([bevy_tasks README](https://github.com/bevyengine/bevy/tree/main/crates/bevy_tasks)).
3. **Rendering**
   - Mesh each chunk via compute shader or CPU mesher per frame; upload using asynchronous asset pipeline (`RenderAssetUsages::REQUIRES_ASSET_LOADING`).
4. **Testing**
   - Validate chunk stepping using `cargo test --all-targets` with deterministic seeds.

## Stage 1 — Building / City Scale (~1 km)
1. **Chunk Paging**
   - Swap dense arrays into a paging layer keyed by `IVec3` chunk coordinates (hash map or `slotmap`). Stream chunks using background tasks.
2. **Gravity Core**
   - Introduce global resource `PlanetCenter: DVec3` and compute gravity vector per voxel: `let dir = (planet_center - world_pos).normalize();` apply to particle/physics proxies.
3. **Performance Controls**
   - Implement budget monitor: track average ms per chunk update; reduce `SimulationSpeed` or update subsets (e.g., even/odd chunk shells) when exceeding `target_ms`.
4. **Visualization**
   - Add instanced rendering per material state to keep entity count low; emit GPU buffers grouped by chunk.
5. **Persistence**
   - Serialize chunk arrays to disk when evicted; store metadata with version, seed, and simulation tick for reproducible reloads.

## Stage 2 — Regional Scale (~100 km)
1. **Adopt `big_space` Coordinates**
   - Add dependency `big_space = { version = "0.10", features = ["bevy"] }`.
   - Register plugin and components:
     ```rust
     app.add_plugins(BigSpacePlugin::default())
         .add_systems(Startup, setup_space);

     fn setup_space(mut commands: Commands) {
         commands.spawn((BigSpace::default(), SpatialBundle::default()));
         commands.spawn((FloatingOrigin, Camera3dBundle::default(), Grid::new(GridPrecision::Int64, 1024.0)));
     }
     ```
     ([Big Space README](https://github.com/aevyrie/big_space/blob/main/README.md#highlights)).
   - Each chunk entity stores `GridCell` + local `Transform`; CA systems operate on `(grid_cell, local_index)` pairs to avoid precision loss ([docs.rs Integer Grid](https://docs.rs/big_space/latest/big_space/#integer-grid)).
2. **Spatial Hashing**
   - Maintain `GridHashMap` for active chunks to fetch neighbors in O(1) ([docs.rs Quick Reference](https://docs.rs/big_space/latest/big_space/)).
   - Use hashed partitions to parallelize CA updates by independent regions.
3. **Morton Ordering with `ilattice`**
   - Add `ilattice = { version = "0.4", features = ["glam"] }` for cache-friendly Morton indexing of chunks and intra-chunk tiles.
   - Store a `MortonKey(u64)` component generated via `MortonEncoder3D::encode(chunk_coords)` to stabilize streaming order and GPU buffer packing.
   - Decode keys with `morton_decode3d` when scheduling neighbor updates to avoid precision loss while still benefiting from Z-order locality.
4. **Level of Detail**
   - Downsample chunk data into `8³` or `16³` macro cells for mid-distance; update LODs asynchronously and swap into impostor meshes.
5. **Time Dilation**
   - Introduce scheduler that advances different latitudinal bands on alternating frames to cap per-frame cost; ensure CA remains stable by using semi-implicit integration for diffusive rules.

## Stage 3 — Planetary Scale (~6,000 km radius)
1. **Hierarchical Storage**
   - Combine chunk paging with an overlay `oktree` for sparse phenomena (storms, fractures). Use `Octree::from_aabb_with_capacity` to track sparse activations ([docs.rs example](https://docs.rs/oktree/0.4.1/oktree/#example)).
   - Store dense mantle in compressed bricks (e.g., `RLE` or `SparseSet`) and hydrate into active chunks near player or fracture front.
   - Reuse `ilattice` Morton keys to map bricks within streaming caches and to align octree leaves with chunk clipmap tiers.
2. **Gravity & Physics**
   - Compute gravity vector in high-precision space: `let offset = big_space.absolute_position(entity);` then accelerate toward center.
   - Integrate with physics engine (Rapier or custom) by converting to local coordinates each frame while using double precision for calculations.
3. **Planet Slicing**
   - For real-time cuts, identify intersected chunks via `oktree` ray queries; re-mesh on worker threads.
   - Use compute shaders to carve geometry: upload plane equation, run compute pipeline that updates voxel states and rebuilds mesh buffers.
4. **Streaming & Networking**
   - Prefetch ahead-of-time using orbital prediction: compute future camera grid cells using velocity and gravity, request data from disk or network.
   - Serialize states with chunk diffs plus `GridCell` metadata for deterministic reassembly.
5. **Visualization**
   - Implement atmospheric scattering shader and horizon-based culling. For far side of planet, use spherical harmonic approximation fed by aggregated CA metrics.

## Stage 4 — Planetary Optimization Loop
1. **Performance Budgeting**
   - Target frame budget of 16 ms. Allocate 6 ms for CA, 4 ms for meshing, 4 ms for rendering, 2 ms margin.
   - If CA exceeds budget, reduce simulation frequency or resolution (dynamic chunk size) via `SimulationSpeed` resource.
2. **GPU Memory Forecast (16 GB)**
   - Reserve 4 GB for render targets/textures, 1 GB for uniform/storage buffers, leaving ~11 GB for chunk meshes.
   - At 1 MB per chunk mesh (e.g., `32³` voxels with position/normal/color), ~11k chunks can be resident simultaneously. Use streaming + compression for additional coverage.
3. **Tooling**
   - Build profiling overlays to visualize chunk budgets, task latency, and recenter frequency.
   - Add automated tests for chunk streaming, recenter correctness, and CA determinism across floating-origin transitions.
4. **Library Vetting**
   - Use `ilattice` for Morton math; avoid pulling `building-blocks` into the MVP due to maintenance hiatus. Instead, port its clipmap and compression concepts into bespoke ECS systems, keeping Bevy integration first-class while retaining the option to prototype against the crate off-branch.

## Stage 5 — Shipping Checklist
- Validate deterministic replay across saves.
- Stress-test by slicing the planet repeatedly; monitor rebuild latency and ensure simulation slowdown remains smooth.
- Document configuration knobs (chunk size, LOD radius, simulation budget) for tuning to different GPUs.

By following these stages, the engine progresses from a room-scale automaton to a planetary simulation that respects precision, performance, and memory constraints while integrating `big_space` and `oktree` effectively.
