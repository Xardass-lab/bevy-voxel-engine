# Scaling Strategies for Planetary Cellular Automata

## Engine-Level Optimizations
- **Profile configs**: Enable dependency optimizations even in dev builds to keep Bevy responsive without losing debug assertions ([Bevy Cheatbook - Performance](https://bevy-cheatbook.github.io/pitfalls/performance.html)). Suggested `Cargo.toml` snippet:
  ```toml
  [profile.dev]
  opt-level = 1
  [profile.dev.package."*"]
  opt-level = 3
  ```
- **Task scheduling**: Offload CA updates to `bevy_tasks::ComputeTaskPool` or dedicated `TaskPoolBuilder` instances; the task API supports scoped parallel iteration and work stealing ([bevy_tasks README](https://github.com/bevyengine/bevy/tree/main/crates/bevy_tasks)).
- **System ordering**: Use `bevy::ecs::schedule::Schedule` with custom sets (e.g., `UpdateSet::Simulation`) so CA updates run before rendering recenter events from `big_space`.

## Spatial Partitioning
- Combine chunked voxel bricks (e.g., `32³` or `64³`) with higher-level spatial structures:
  - `GridHashMap` from `big_space` for O(1) neighbor discovery across chunk boundaries.
  - Optional `oktree` overlays for sparsely active regions (e.g., atmosphere, weather) to reduce memory footprint.
- Maintain multi-resolution LOD chain: near-field uses dense simulation, mid-field uses aggregated chunk states (averaged or downsampled), far-field uses analytical/texture-driven approximations.

## Time Dilation & Simulation Budgeting
- Track average CA step cost (ms) and dynamically scale simulation rate when exceeding frame budget. Provide `SimulationSpeed` resource that lerps toward target `dt` to avoid abrupt slowdowns.
- Permit "batch" updates by splitting the planet into rings or face patches updated on alternating frames; ensures consistent progress without blocking render thread.

## Rendering Considerations
- Use instanced meshes or compute-driven mesh generation for active voxels instead of individual entities; mesh generation should operate per chunk.
- Implement view-dependent LOD: skip CA meshing for chunks outside a camera-dependent radius, fallback to impostor textures or GPU-generated spheres.
- Employ async GPU uploads (via `RenderAssetUsages::REQUIRES_ASSET_LOADING`) to stream chunk meshes without stalling the main thread.

## Persistence & Streaming
- Serialize CA state per chunk using `GridCell` indices plus local dense arrays for compatibility with `big_space` coordinates.
- Implement background streaming threads that prefetch neighbor chunks based on camera velocity and gravity vector, warming caches before the player arrives.

## Failure Modes & Mitigations
- **Floating origin jitter**: recenter events can cause physics jitter; double-buffer CA-to-physics transforms and apply smoothing.
- **GPU memory blow-up**: For a 16 GB GPU, reserve <12 GB for chunk meshes/textures to leave headroom for swapchain; this equates to ~12k active chunks at 1 MB each. Use compression or procedural regeneration for far-field data.
- **Workload spikes when splitting planets**: precompute fracture planes and reuse mesh topology where possible; stream cut geometry via compute shaders to keep CPU cost bounded.
