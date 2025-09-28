# Building Blocks Crate Assessment

## Overview
- **Repository**: https://github.com/bonsairobo/building-blocks
- **Status**: Maintenance mode; author recommends migrating to smaller successor crates focused on slice-based APIs backed by the feldspar project.
- **Scope**: Provides voxel-focused data structures, LOD clipmaps, chunk storage, procedural sampling helpers, greedy and Surface Nets meshing, chunk databases with compression, and spatial queries.

## Strengths
- Ready-made chunk trees with split/merge events for clipmap-driven LOD and streaming.
- Multiple meshing algorithms (greedy, Surface Nets) usable on CPU workers with configurable voxel sizes.
- `ChunkDb` abstraction for compressed persistence with `sled`, supporting LZ4/Snappy backends and configurable features.
- Rich documentation and examples covering sampling SDFs, chunk paging, and clipmap management.

## Weaknesses / Risks
- Maintenance hiatus makes long-term support uncertain; upstream recommends new crates in the author's "my-stack" list instead.
- API centered around bespoke `Array`/`Extent` types may conflict with existing ECS-centric storage, leading to conversion overhead.
- Heavy dependency surface (meshing, compression, pathfinding) when enabling default features; requires careful feature gating for WASM or minimal builds.

## Applicability to Planetary CA MVP
- **Chunk storage & paging**: `ChunkTree` could prototype clipmap paging before custom solution, but lack of maintenance makes it risky for core MVP. Favor integrating concepts rather than depending on crate binaries.
- **Meshing**: Greedy meshing implementation can serve as reference for GPU/compute rewrite, but shipping dependency into MVP is optional.
- **Compression**: `ChunkDb` demonstrates chunk diff persistence; evaluate porting design to in-house storage or using smaller crates that remain maintained.

## Integration Notes
- If prototyping with the crate, disable default features and enable only required modules to limit dependencies:
  ```toml
  [dependencies]
  building-blocks = { version = "0.7", default-features = false, features = [
      "array", "chunk_map", "chunk_tree", "lod", "mesh"
  ] }
  ```
- Convert between `building_blocks::core::Point3i` and Bevy `IVec3` through `.into()` when the "glam" feature is enabled.
- Run meshing on worker threads and upload results to Bevy using `RenderAsset` implementations to keep render graph decoupled from CPU mesher.

## Recommendation
Treat Building Blocks as a design reference and optional prototype helper. For the MVP planetary CA, replicate core ideas (clipmaps, chunk compression) with maintained, ECS-friendly libraries or in-house implementations to avoid lock-in to an unmaintained dependency.
