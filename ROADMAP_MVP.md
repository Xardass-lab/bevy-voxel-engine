# Planetary Cellular Automata Roadmap (Room → Planet)

This roadmap outlines the concrete engine work needed to evolve the current Bevy voxel prototype into a destructible planet-scale cellular automaton. Every stage includes the exact glue code or crate integrations that hook into the codebase under `src/simulation.rs`, `src/lib.rs`, and `src/voxel_pipeline` so the plan can be implemented without placeholders.

## Stage 0 — Room-Scale Prototype
1. **Data Model**
   - Keep chunk state in the `ChunkCells`/`ChunkCellsNext` components defined in `src/simulation.rs`. Each chunk owns a `Box<[AutomataState]>` sized to `CHUNK_VOLUME` (32³). `AutomataState` mirrors the renderer’s `R16Uint` layout, so you can copy GPU buffers directly via `ChunkCells::write_from_packed` and `ChunkCells::to_packed_vec` without repacking material/flag bytes.
   - Instantiate chunks with the provided `ChunkBundle::new(coords)` constructor so entities always carry `ChunkKey`, `ChunkCells`, and `ChunkCellsNext` together. Use `ChunkBundle::from_generator` when seeding test rooms procedurally.
   - Register the `CellularAutomataPlugin` from `src/lib.rs` to bring in the fixed-step scheduler (`SimulationSet`) and runtime resources (`SimulationSpeed`, `SimulationBudget`, `SimulationClock`, `ChunkSnapshots`, and `ChunkIndex`).
2. **Simulation Loop**
   - Systems already exist in `src/simulation.rs`: `tick_simulation` accumulates delta time, `snapshot_chunks` copies read-only state into `ChunkSnapshots`, `step_chunks` iterates up to `MAX_STEPS_PER_FRAME` steps, and `apply_next_cells` swaps buffers. Wire them ahead of rendering by adding the plugin, or schedule custom rules in the `SimulationSet::Step` set:
     ```rust
     app.add_plugins(CellularAutomataPlugin)
         .add_systems(Update, custom_room_rule.in_set(SimulationSet::Step));

     fn custom_room_rule(
         rule: Res<AutomataRule>,
         mut next_chunks: Query<&mut ChunkCellsNext>,
     ) {
         for mut buffer in &mut next_chunks {
             // mutate `buffer.as_mut_slice()` here before `apply_next_cells` runs
         }
     }
     ```
   - Adjust playback speed at runtime through `SimulationSpeed` (e.g., `commands.insert_resource(SimulationSpeed { factor: 2.0, ..default() })`). The budget controller automatically clamps the factor based on the moving average recorded in `SimulationBudget`.
   - `ChunkScratchpad` pre-allocates per-chunk buffers so `step_chunks` can reuse memory across fixed-step iterations; call `world.resource_mut::<ChunkScratchpad>()` when writing custom stepping logic to avoid allocating temporary `Vec<AutomataState>` buffers.
3. **Rendering / GPU Interop**
   - When CPU rules change voxels, push the data into the renderer by calling `ChunkCells::to_packed_vec()` and uploading to the existing compute pipelines under `src/voxel_pipeline/compute`. Use the shader handles registered in `voxel_pipeline::compute::add_compute_pipelines` to trigger rebuilds.
   - For room-scale debugging, reuse `VoxelizationBundle` and `VoxelCameraBundle` from `src/lib.rs` to spawn visualizers that track the CA output.
4. **Testing**
   - The regression tests in `src/simulation.rs::tests` (Morton uniqueness, cross-chunk neighbor lookup, accumulator draining) document the base invariants. Extend them with scenario-specific checks and run with `cargo test --lib` to guarantee determinism.

## Stage 1 — Building / City Scale (~1 km)
1. **Chunk Paging**
   - Back CA chunks with a `HashMap<IVec3, ChunkBundle>` or `slotmap::SlotMap` keyed by the `ChunkKey.coords`. When evicting chunks, serialize `ChunkCells::to_packed_vec()` and store the metadata `(coords, SimulationClock::accumulator, tick)` so reloading resumes deterministically.
   - Drive background streaming via `ComputeTaskPool::scope` (already imported in `src/simulation.rs`) to hydrate chunks without blocking the main thread.
2. **Gravity Core**
   - Introduce a `PlanetCenter: DVec3` resource and apply gravity within physics or particle systems: `let gravity = (planet_center - world_pos).normalize() * g;`. `VoxelPhysics` components in `src/lib.rs` already carry `gravity` fields—update them when moving chunks.
3. **Performance Controls**
   - Monitor the moving average from `SimulationBudget::rolling_ms`. If it exceeds `target_ms`, either lower `SimulationSpeed.factor` or update subsets of chunks by filtering on `ChunkKey.morton % n` for shell-based throttling.
4. **Visualization**
   - Group render uploads per material by reading `AutomataState::material()` while meshing to keep draw calls low. The compute pipelines under `src/voxel_pipeline/compute/*.rs` already batch WGSL shader uploads; extend them with material sorting before dispatch.
5. **Persistence**
   - Save chunks when removed from `ChunkIndex` and reload through `ChunkCells::write_from_packed`. Keep snapshots consistent by reinserting entries into `ChunkSnapshots` before the next `step_chunks` iteration.

## Stage 2 — Regional Scale (~100 km)
1. **Adopt `big_space` Coordinates**
   - Add `big_space = { version = "0.10", features = ["bevy"] }` to `Cargo.toml`, register `BigSpacePlugin`, and attach a `FloatingOrigin` camera (see `docs/big_space_research.md`). Store each chunk’s `GridCell` alongside its `ChunkKey` so CA systems can map between integer grid cells and local voxel indices with millimeter precision.
2. **Spatial Hashing**
   - Replace the plain `HashMap` inside `ChunkIndex` with `big_space::GridHashMap` to fetch neighboring chunks in O(1). When calling `count_active_neighbors`, translate offsets using `GridCell::neighbor(offset)` before querying `ChunkSnapshots`.
3. **Morton Ordering with `ilattice`**
   - Pull in `ilattice = { version = "0.4", features = ["glam"] }` and replace `morton_encode`/`part1by2` with `MortonEncoder3D::encode` for clarity. Cache `MortonKey(u64)` components and use them to sort streaming requests or GPU upload queues.
4. **Level of Detail**
   - Generate downsampled macro cells (8³ or 16³) on worker threads and render them via instancing while detailed chunks load. Record the macro state in secondary textures so compute shaders can blend transitions.
5. **Time Dilation**
   - Advance latitudinal bands on alternating frames by checking `ChunkKey.coords.y % 2`. Combine with `SimulationSpeed` to keep the global clock smooth while letting distant regions update less frequently.

## Stage 3 — Planetary Scale (~6,000 km radius)
1. **Hierarchical Storage**
   - Layer an `oktree::Octree` over the chunk pager (see `docs/oktree_research.md`). Keep dense mantle data in compressed bricks (RLE or sparse sets) and hydrate them into active `ChunkCells` when the player or fracture front approaches. Map octree leaves to chunk morton keys to keep streaming cache-friendly.
2. **Gravity & Physics**
   - Use `big_space::BigSpace` to query absolute positions: `let offset = big_space.absolute_position(entity);`. Feed that into physics integration (`VoxelPhysics::gravity`) using double precision before projecting back to local chunk coordinates.
3. **Planet Slicing**
   - Cast rays through the octree to find intersected chunks, then enqueue compute shader cuts. Upload plane equations or boolean volumes to the compute pipelines (see `src/voxel_pipeline/compute/rebuild.rs`) and write results back with `ChunkCells::write_from_packed`.
4. **Streaming & Networking**
   - Predict camera motion from velocity + gravity, prefetch upcoming `GridCell`s, and stream chunk diffs over the network. Persist `(GridCell, AutomataState)` deltas so server and client remain deterministic.
5. **Visualization**
   - Integrate atmospheric scattering and horizon culling; feed aggregated CA metrics into spherical harmonics for the planet’s far side while detailed chunks remain local.

## Stage 4 — Planetary Optimization Loop
1. **Performance Budgeting**
   - Maintain a 16 ms frame budget: 6 ms CA, 4 ms meshing, 4 ms rendering, 2 ms margin. If CA overruns, dial back `SimulationSpeed.factor` or enlarge `CHUNK_EDGE` temporarily for low-activity regions.
2. **GPU Memory Forecast (16 GB)**
   - Reserve ~4 GB for render targets and 1 GB for uniform/storage buffers, leaving 11 GB for meshes. At ~1 MB per chunk mesh you can keep ≈11 000 active chunks. Stream additional chunks through compressed brick caches.
3. **Tooling**
   - Build profiling overlays showing chunk timings, background task latency, and floating-origin recenter counts. Log `SimulationBudget::rolling_ms` and chunk step durations to quickly spot regressions.
4. **Library Vetting**
   - Use `ilattice` for Morton math and stay off `building-blocks` in the MVP (documented in `docs/building_blocks_research.md`). Port required clipmap/compression ideas directly into Bevy ECS systems for long-term maintainability.

## Stage 5 — Shipping Checklist
- Validate deterministic replay across saves by capturing `SimulationClock` state and all chunk diffs.
- Stress-test real-time slicing against the octree-backed mesher; ensure `SimulationSpeed` gracefully slows instead of stalling.
- Document tuning knobs (chunk size, LOD radius, simulation budget, streaming thresholds) so the engine can be configured for a range of GPUs.

By following these stages the engine scales from a room-sized automaton to a planetary simulation while preserving determinism, GPU/CPU interoperability, and realistic gravity oriented toward the planet core.
