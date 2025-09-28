use crate::Flags;
use bevy::{
    ecs::schedule::SystemSet,
    prelude::*,
    utils::{HashMap, Instant},
};
use std::sync::Arc;

/// Edge length of a simulation chunk in voxels.
pub const CHUNK_EDGE: i32 = 32;
/// Number of voxels contained inside a chunk.
pub const CHUNK_VOLUME: usize =
    (CHUNK_EDGE as usize) * (CHUNK_EDGE as usize) * (CHUNK_EDGE as usize);
/// Fixed time step used to advance the cellular automata.
pub const FIXED_STEP_SECONDS: f32 = 1.0 / 60.0;
/// Bias applied to chunk coordinates before Morton encoding.
const MORTON_BIAS: i32 = 1 << 20;

/// Packed material/flag state stored per voxel.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct AutomataState {
    encoded: u16,
}

impl AutomataState {
    /// Constructs a state from palette material and flag byte.
    pub const fn from_components(material: u8, flags: u8) -> Self {
        Self {
            encoded: ((flags as u16) << 8) | material as u16,
        }
    }

    /// Constructs a state from the packed 16-bit value stored in GPU textures.
    pub const fn from_packed(encoded: u16) -> Self {
        Self { encoded }
    }

    /// Returns the packed 16-bit representation of the voxel.
    pub const fn to_packed(self) -> u16 {
        self.encoded
    }

    /// Returns the palette material index stored in the low byte.
    pub const fn material(self) -> u8 {
        (self.encoded & 0x00FF) as u8
    }

    /// Returns the voxel flags stored in the high byte.
    pub const fn flags(self) -> u8 {
        (self.encoded >> 8) as u8
    }

    /// Returns true when the voxel stores no material or flags.
    pub const fn is_empty(self) -> bool {
        self.encoded == 0
    }

    /// Returns true when the voxel contains any material.
    pub const fn is_solid(self) -> bool {
        self.material() != 0
    }

    /// Returns true when the voxel participates in the automata rule set.
    pub const fn is_alive(self) -> bool {
        (self.flags() & Flags::AUTOMATA_FLAG) != 0 && self.is_solid()
    }

    /// Returns true when the voxel should be treated as immutable geometry.
    pub const fn is_static(self) -> bool {
        self.is_solid() && !self.is_alive()
    }

    /// Replaces the palette material and returns the new state.
    pub const fn with_material(self, material: u8) -> Self {
        Self::from_components(material, self.flags())
    }

    /// Replaces the flag byte and returns the new state.
    pub const fn with_flags(self, flags: u8) -> Self {
        Self::from_components(self.material(), flags)
    }

    /// Returns the (material, flags) tuple for interoperability helpers.
    pub const fn to_components(self) -> (u8, u8) {
        (self.material(), self.flags())
    }
}

impl From<u16> for AutomataState {
    fn from(value: u16) -> Self {
        Self::from_packed(value)
    }
}

impl From<AutomataState> for u16 {
    fn from(value: AutomataState) -> Self {
        value.to_packed()
    }
}

impl From<(u8, u8)> for AutomataState {
    fn from((material, flags): (u8, u8)) -> Self {
        Self::from_components(material, flags)
    }
}

/// Resource controlling the simulation playback speed.
#[derive(Resource, Debug, Clone, Copy)]
pub struct SimulationSpeed {
    /// Multiplier applied to the fixed simulation step.
    pub factor: f32,
    /// Lower clamp to keep the simulation responsive under load.
    pub min_factor: f32,
    /// Upper clamp to avoid runaway acceleration.
    pub max_factor: f32,
}

impl Default for SimulationSpeed {
    fn default() -> Self {
        Self {
            factor: 1.0,
            min_factor: 0.1,
            max_factor: 4.0,
        }
    }
}

impl SimulationSpeed {
    fn apply_budget_feedback(&mut self, budget: &SimulationBudget) {
        if budget.rolling_ms > budget.target_ms {
            self.factor = (self.factor * 0.9).max(self.min_factor);
        } else if budget.rolling_ms < budget.target_ms * 0.5 {
            self.factor = (self.factor * 1.05).min(self.max_factor);
        }
    }
}

/// Tracks how much CPU time the simulation consumed and adjusts playback speed targets.
#[derive(Resource, Debug, Clone, Copy)]
pub struct SimulationBudget {
    /// Maximum milliseconds budgeted per fixed-step update.
    pub target_ms: f32,
    smoothing: f32,
    /// Exponential moving average of recent step times.
    pub rolling_ms: f32,
}

impl Default for SimulationBudget {
    fn default() -> Self {
        Self {
            target_ms: 6.0,
            smoothing: 0.2,
            rolling_ms: 0.0,
        }
    }
}

impl SimulationBudget {
    pub fn record_step(&mut self, elapsed_ms: f32) {
        if self.rolling_ms == 0.0 {
            self.rolling_ms = elapsed_ms;
        } else {
            self.rolling_ms += self.smoothing * (elapsed_ms - self.rolling_ms);
        }
    }
}

/// Fixed-step clock so the automata runs deterministically regardless of framerate.
#[derive(Resource, Debug, Clone, Copy)]
pub struct SimulationClock {
    accumulator: f32,
    /// Number of steps requested during the current frame.
    pub steps_requested: u32,
    /// Whether the step for this frame has completed.
    pub executed_step: bool,
}

impl Default for SimulationClock {
    fn default() -> Self {
        Self {
            accumulator: 0.0,
            steps_requested: 0,
            executed_step: false,
        }
    }
}

/// Birth/survival rule configured for the MVP.
#[derive(Resource, Debug, Clone)]
pub struct AutomataRule {
    pub birth: Vec<u8>,
    pub survive: Vec<u8>,
    /// Palette index used when birthing a new automata voxel.
    pub birth_material: u8,
    /// Flags applied to newly created automata voxels.
    pub birth_flags: u8,
    /// State applied to voxels that fall out of the rule (typically empty space).
    pub inactive_state: AutomataState,
}

impl Default for AutomataRule {
    fn default() -> Self {
        // Use a 3D Life variant (B5/S45) that produces interesting structures.
        Self {
            birth: vec![5],
            survive: vec![4, 5],
            birth_material: 1,
            birth_flags: Flags::AUTOMATA_FLAG,
            inactive_state: AutomataState::default(),
        }
    }
}

impl AutomataRule {
    #[inline]
    fn alive_template(&self) -> AutomataState {
        AutomataState::from_components(self.birth_material, self.birth_flags | Flags::AUTOMATA_FLAG)
    }

    #[inline]
    fn next_state(&self, current: AutomataState, neighbors: u8) -> AutomataState {
        if current.is_static() {
            return current;
        }

        if current.is_alive() {
            if self.survive.contains(&neighbors) {
                let mut flags = current.flags() | Flags::AUTOMATA_FLAG;
                flags |= self.birth_flags & !Flags::AUTOMATA_FLAG;
                current.with_flags(flags)
            } else {
                self.inactive_state
            }
        } else if self.birth.contains(&neighbors) {
            self.alive_template()
        } else {
            self.inactive_state
        }
    }
}

/// Component storing the Morton key for a chunk along with its integer coordinates.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChunkKey {
    pub coords: IVec3,
    pub morton: u64,
}

impl ChunkKey {
    pub fn new(coords: IVec3) -> Self {
        Self {
            morton: morton_encode(coords),
            coords,
        }
    }
}

/// Component containing the active state for every cell in a chunk.
#[derive(Component, Clone)]
pub struct ChunkCells {
    data: Box<[AutomataState]>,
}

impl ChunkCells {
    pub fn filled(value: AutomataState) -> Self {
        Self {
            data: vec![value; CHUNK_VOLUME].into_boxed_slice(),
        }
    }

    pub fn from_generator<F>(mut generator: F) -> Self
    where
        F: FnMut(IVec3) -> AutomataState,
    {
        let mut data = Vec::with_capacity(CHUNK_VOLUME);
        for x in 0..CHUNK_EDGE {
            for y in 0..CHUNK_EDGE {
                for z in 0..CHUNK_EDGE {
                    data.push(generator(IVec3::new(x, y, z)));
                }
            }
        }

        Self {
            data: data.into_boxed_slice(),
        }
    }

    #[inline]
    pub fn as_slice(&self) -> &[AutomataState] {
        &self.data
    }

    #[inline]
    pub fn clone_box(&self) -> Box<[AutomataState]> {
        self.data.clone()
    }

    #[inline]
    pub fn write_from_slice(&mut self, data: &[AutomataState]) {
        self.data.as_mut().copy_from_slice(data);
    }

    /// Writes packed GPU-compatible values into the chunk.
    pub fn write_from_packed(&mut self, data: &[u16]) {
        debug_assert_eq!(data.len(), self.data.len());
        for (dst, &packed) in self.data.iter_mut().zip(data.iter()) {
            *dst = AutomataState::from(packed);
        }
    }

    /// Returns the packed GPU representation of this chunk's voxels.
    pub fn to_packed_vec(&self) -> Vec<u16> {
        self.data
            .iter()
            .copied()
            .map(AutomataState::to_packed)
            .collect()
    }
}

impl Default for ChunkCells {
    fn default() -> Self {
        Self::filled(AutomataState::default())
    }
}

/// Component used as the write-target for the next CA state.
#[derive(Component, Clone)]
pub struct ChunkCellsNext {
    data: Box<[AutomataState]>,
}

impl ChunkCellsNext {
    pub fn zeros() -> Self {
        Self {
            data: vec![AutomataState::default(); CHUNK_VOLUME].into_boxed_slice(),
        }
    }

    #[inline]
    pub fn as_mut_slice(&mut self) -> &mut [AutomataState] {
        &mut self.data
    }

    #[inline]
    pub fn as_slice(&self) -> &[AutomataState] {
        &self.data
    }
}

impl Default for ChunkCellsNext {
    fn default() -> Self {
        Self::zeros()
    }
}

/// Bundle wiring together the data necessary to simulate a chunk.
#[derive(Bundle)]
pub struct ChunkBundle {
    pub key: ChunkKey,
    pub cells: ChunkCells,
    pub next: ChunkCellsNext,
}

impl ChunkBundle {
    pub fn new(coords: IVec3) -> Self {
        Self {
            key: ChunkKey::new(coords),
            cells: ChunkCells::default(),
            next: ChunkCellsNext::default(),
        }
    }

    pub fn from_generator<F>(coords: IVec3, generator: F) -> Self
    where
        F: FnMut(IVec3) -> AutomataState,
    {
        Self {
            key: ChunkKey::new(coords),
            cells: ChunkCells::from_generator(generator),
            next: ChunkCellsNext::default(),
        }
    }
}

/// Resource exposing a fast lookup from chunk coordinates to ECS entity.
#[derive(Resource, Default, Debug)]
pub struct ChunkIndex {
    entries: HashMap<IVec3, Entity>,
}

impl ChunkIndex {
    pub fn entity(&self, coords: IVec3) -> Option<Entity> {
        self.entries.get(&coords).copied()
    }

    fn rebuild(&mut self, entries: impl Iterator<Item = (IVec3, Entity)>) {
        self.entries.clear();
        for (coords, entity) in entries {
            self.entries.insert(coords, entity);
        }
    }
}

/// Snapshot of chunk data used to evaluate the next automata state without aliasing.
#[derive(Resource, Default, Debug)]
pub struct ChunkSnapshots {
    map: HashMap<IVec3, Arc<[AutomataState]>>,
}

impl ChunkSnapshots {
    #[inline]
    pub fn get(&self, coords: IVec3) -> Option<&[AutomataState]> {
        self.map.get(&coords).map(|arc| arc.as_ref())
    }

    fn rebuild(&mut self, snapshots: impl Iterator<Item = (IVec3, Arc<[AutomataState]>)>) {
        self.map.clear();
        for (coords, snapshot) in snapshots {
            self.map.insert(coords, snapshot);
        }
    }
}

/// Systems executed by the [`CellularAutomataPlugin`].
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum SimulationSet {
    Tick,
    Snapshot,
    Step,
    Apply,
}

/// Plugin wiring the MVP cellular automata loop into the Bevy schedule.
pub struct CellularAutomataPlugin;

impl Plugin for CellularAutomataPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SimulationSpeed>()
            .init_resource::<SimulationBudget>()
            .init_resource::<SimulationClock>()
            .init_resource::<ChunkIndex>()
            .init_resource::<ChunkSnapshots>()
            .insert_resource(AutomataRule::default())
            .add_systems(First, tick_simulation.in_set(SimulationSet::Tick))
            .add_systems(PreUpdate, snapshot_chunks.in_set(SimulationSet::Snapshot))
            .add_systems(Update, step_chunks.in_set(SimulationSet::Step))
            .add_systems(PostUpdate, apply_next_cells.in_set(SimulationSet::Apply));
    }
}

fn tick_simulation(
    time: Res<Time>,
    mut clock: ResMut<SimulationClock>,
    speed: Res<SimulationSpeed>,
) {
    let delta = time.delta_seconds();
    clock.accumulator += delta * speed.factor;
    clock.steps_requested = 0;
    clock.executed_step = false;

    if clock.accumulator >= FIXED_STEP_SECONDS {
        clock.accumulator -= FIXED_STEP_SECONDS;
        clock.steps_requested = 1;
    }
}

fn snapshot_chunks(
    mut snapshots: ResMut<ChunkSnapshots>,
    mut index: ResMut<ChunkIndex>,
    clock: Res<SimulationClock>,
    query: Query<(Entity, &ChunkKey, &ChunkCells)>,
) {
    if clock.steps_requested == 0 {
        return;
    }

    let len = query.iter().len();
    let mut snapshot_entries = Vec::with_capacity(len);
    let mut index_entries = Vec::with_capacity(len);

    for (entity, key, cells) in query.iter() {
        snapshot_entries.push((key.coords, Arc::from(cells.clone_box())));
        index_entries.push((key.coords, entity));
    }

    snapshots.rebuild(snapshot_entries.into_iter());
    index.rebuild(index_entries.into_iter());
}

fn step_chunks(
    mut clock: ResMut<SimulationClock>,
    mut speed: ResMut<SimulationSpeed>,
    mut budget: ResMut<SimulationBudget>,
    snapshots: Res<ChunkSnapshots>,
    rule: Res<AutomataRule>,
    query: Query<(Entity, &ChunkKey)>,
    cells_query: Query<&ChunkCells>,
    mut next_query: Query<&mut ChunkCellsNext>,
) {
    if clock.steps_requested == 0 {
        return;
    }

    let start = Instant::now();
    let mut results = Vec::with_capacity(query.iter().len());

    for (entity, key) in query.iter() {
        if let Some(snapshot) = snapshots.get(key.coords) {
            let mut buffer = vec![AutomataState::default(); CHUNK_VOLUME];
            step_chunk(snapshot, key.coords, &snapshots, &rule, &mut buffer);
            results.push((entity, buffer));
        } else if let Ok(cells) = cells_query.get(entity) {
            // No snapshot available (chunk added mid-frame); fall back to current cells.
            let mut buffer = vec![AutomataState::default(); CHUNK_VOLUME];
            step_chunk(cells.as_slice(), key.coords, &snapshots, &rule, &mut buffer);
            results.push((entity, buffer));
        }
    }

    for (entity, buffer) in results {
        if let Ok(mut next) = next_query.get_mut(entity) {
            next.as_mut_slice().copy_from_slice(&buffer);
        }
    }

    let elapsed_ms = start.elapsed().as_secs_f32() * 1000.0;
    budget.record_step(elapsed_ms);
    speed.apply_budget_feedback(&budget);
    clock.steps_requested = 0;
    clock.executed_step = true;
}

fn apply_next_cells(
    mut clock: ResMut<SimulationClock>,
    mut query: Query<(&mut ChunkCells, &ChunkCellsNext)>,
) {
    if !clock.executed_step {
        return;
    }

    for (mut cells, next) in query.iter_mut() {
        cells.write_from_slice(next.as_slice());
    }

    clock.executed_step = false;
}

fn step_chunk(
    current_chunk: &[AutomataState],
    coords: IVec3,
    snapshots: &ChunkSnapshots,
    rule: &AutomataRule,
    output: &mut [AutomataState],
) {
    for x in 0..CHUNK_EDGE {
        for y in 0..CHUNK_EDGE {
            for z in 0..CHUNK_EDGE {
                let local = IVec3::new(x, y, z);
                let idx = linear_index(local);
                let neighbors = count_active_neighbors(snapshots, coords, local);
                let current = current_chunk[idx];
                output[idx] = rule.next_state(current, neighbors);
            }
        }
    }
}

fn count_active_neighbors(snapshots: &ChunkSnapshots, chunk_coords: IVec3, local: IVec3) -> u8 {
    let mut count = 0u8;

    for dx in -1..=1 {
        for dy in -1..=1 {
            for dz in -1..=1 {
                if dx == 0 && dy == 0 && dz == 0 {
                    continue;
                }

                let offset = IVec3::new(dx, dy, dz);
                if let Some(value) = sample_cell(snapshots, chunk_coords, local + offset) {
                    if value.is_alive() {
                        count = count.saturating_add(1);
                    }
                }
            }
        }
    }

    count
}

fn sample_cell(
    snapshots: &ChunkSnapshots,
    mut chunk_coords: IVec3,
    mut local: IVec3,
) -> Option<AutomataState> {
    let edge = CHUNK_EDGE;

    if local.x < 0 {
        chunk_coords.x -= 1;
        local.x += edge;
    } else if local.x >= edge {
        chunk_coords.x += 1;
        local.x -= edge;
    }

    if local.y < 0 {
        chunk_coords.y -= 1;
        local.y += edge;
    } else if local.y >= edge {
        chunk_coords.y += 1;
        local.y -= edge;
    }

    if local.z < 0 {
        chunk_coords.z -= 1;
        local.z += edge;
    } else if local.z >= edge {
        chunk_coords.z += 1;
        local.z -= edge;
    }

    if let Some(chunk) = snapshots.get(chunk_coords) {
        let index = linear_index(local);
        Some(chunk[index])
    } else {
        None
    }
}

#[inline]
fn linear_index(local: IVec3) -> usize {
    let edge = CHUNK_EDGE as usize;
    (local.x as usize * edge * edge) + (local.y as usize * edge) + local.z as usize
}

#[inline]
fn morton_encode(coords: IVec3) -> u64 {
    let x = (coords.x + MORTON_BIAS) as u64;
    let y = (coords.y + MORTON_BIAS) as u64;
    let z = (coords.z + MORTON_BIAS) as u64;

    part1by2(x) | (part1by2(y) << 1) | (part1by2(z) << 2)
}

#[inline]
fn part1by2(mut n: u64) -> u64 {
    n &= 0x1f_ffff;
    n = (n | (n << 32)) & 0x1f00_0000_00ff_ff;
    n = (n | (n << 16)) & 0x1f00_00ff_0000_ff;
    n = (n | (n << 8)) & 0x100f_00f0_0f00_f00f;
    n = (n | (n << 4)) & 0x10c3_0c30_c30c_30c3;
    n = (n | (n << 2)) & 0x1249_2492_4924_9249;
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Flags;
    use std::collections::HashSet;

    #[test]
    fn morton_keys_are_unique_for_local_region() {
        let mut seen = HashSet::new();
        for x in -2..=2 {
            for y in -2..=2 {
                for z in -2..=2 {
                    let key = morton_encode(IVec3::new(x, y, z));
                    assert!(seen.insert(key));
                }
            }
        }
    }

    #[test]
    fn neighbor_lookup_crosses_chunk_boundary() {
        let mut snapshots = ChunkSnapshots::default();
        let mut map = HashMap::default();

        let mut center = vec![AutomataState::default(); CHUNK_VOLUME];
        center[linear_index(IVec3::new(CHUNK_EDGE - 1, CHUNK_EDGE - 1, CHUNK_EDGE - 1))] =
            AutomataState::from_components(1, Flags::AUTOMATA_FLAG);
        map.insert(IVec3::ZERO, Arc::from(center.into_boxed_slice()));

        let mut neighbor = vec![AutomataState::default(); CHUNK_VOLUME];
        neighbor[linear_index(IVec3::new(0, 0, 0))] =
            AutomataState::from_components(1, Flags::AUTOMATA_FLAG);
        map.insert(IVec3::new(1, 1, 1), Arc::from(neighbor.into_boxed_slice()));

        snapshots.map = map;

        let count = count_active_neighbors(
            &snapshots,
            IVec3::ZERO,
            IVec3::new(CHUNK_EDGE - 1, CHUNK_EDGE - 1, CHUNK_EDGE - 1),
        );
        assert_eq!(count, 1);
    }
}
