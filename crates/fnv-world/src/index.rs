//! Which cells belong to which worldspace, and which references to which cell, across a
//! load order.
//!
//! Plugin structure (docs/spec-notes/esm.md, world.md): a worldspace's records sit in a
//! world-children group (type 1) labelled with the WRLD FormID; exterior cells sit in
//! block/sub-block groups inside it, each CELL record followed by a cell-children group
//! (type 6) holding persistent (8) and temporary (9) groups of references and the cell's
//! LAND. Interior cells sit in the top-level CELL group. Group labels are FormIDs local to
//! the plugin. A later plugin overrides a cell or reference by repeating it with the same
//! FormID; the last definition in load order wins, including which cell a reference is in.

use crate::fields::{formid_of, global, num, Fields};
use fnv_esm::raw::{group_type, visit_records};
use fnv_esm::{GroupHeader, LoadOrder, Schema};
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone, Default)]
pub struct Worldspace {
    pub form_id: u32,
    pub edid: Option<String>,
    pub name: Option<String>,
    pub parent: Option<u32>,
    /// WRLD PNAM: which data the worldspace takes from its parent ([`parent_use`]).
    /// [`WorldIndex::build`] has already followed it for the land, the default heights,
    /// the water type and the climate ([`WorldIndex::take_parent_data`]).
    pub parent_flags: u16,
    /// WRLD DATA flags (bit 0 Small World, 4 No LOD Water, … [documented: xEdit WRLD]).
    pub data_flags: u8,
    pub default_land_height: Option<f32>,
    pub default_water_height: Option<f32>,
    /// WRLD CNAM climate and NAM2 water.
    pub climate: Option<u32>,
    pub water: Option<u32>,
    /// WRLD XEZN: the encounter zone of the worldspace's cells that name none
    /// (docs/spec-notes/encounter-zones.md §1).
    pub encounter_zone: Option<u32>,
    /// The cell placed directly in the world-children group (not in a block); it holds
    /// the worldspace's persistent references.
    pub persistent_cell: Option<u32>,
    /// Exterior cells by grid coordinate.
    pub grid: BTreeMap<(i32, i32), u32>,
}

#[derive(Debug, Clone, Default)]
pub struct Cell {
    pub form_id: u32,
    pub edid: Option<String>,
    pub name: Option<String>,
    /// Worldspace for exterior cells (and the persistent cell); `None` for interiors.
    pub world: Option<u32>,
    pub grid: Option<(i32, i32)>,
    /// CELL DATA flags (bit 0: interior, bit 1: has water).
    pub data_flags: u8,
    /// CELL XCLW, when present.
    pub water_height: Option<f32>,
    /// CELL XCWT water type, when present.
    pub water_type: Option<u32>,
    /// CELL XEZN: the cell's encounter zone (docs/spec-notes/encounter-zones.md §1).
    pub encounter_zone: Option<u32>,
    /// Load-order index of the plugin with the winning CELL record.
    pub plugin: usize,
    /// References whose winning placement is in this cell, by FormID.
    pub refs: Vec<u32>,
    /// The cell's LAND record. In a worldspace that uses its parent's land data it is the
    /// parent's, of the cell on the same grid square, where the parent has one, the
    /// cell's own LAND or not ([`Cell::land_of_parent`] says so).
    pub land: Option<u32>,
    /// `land` is the parent worldspace's ([`WorldIndex::take_parent_data`]).
    pub land_of_parent: bool,
}

impl Cell {
    pub fn is_interior(&self) -> bool {
        self.data_flags & 1 != 0
    }

    /// CELL DATA bit 1 [documented: xEdit "Has water"].
    pub fn has_water(&self) -> bool {
        self.data_flags & 2 != 0
    }

    /// CELL DATA bit 3 [documented: xEdit "No LOD Water"]: set on 3,967 cells, all
    /// of them Zion Canyon's, in the masters [observed].
    pub fn no_lod_water(&self) -> bool {
        self.data_flags & 8 != 0
    }

    /// Height of the cell's water surface: none without the "has water" flag; XCLW when it
    /// holds a real height; otherwise, outdoors, the worldspace's default water height
    /// (WRLD DNAM), passed as `world_default` [hypothesis for the fallback, world.md §6:
    /// 15,963 WastelandNV cells store the 3.4e38 sentinel]. The rule the renderer draws
    /// water with and the physics swims in (physics.md §6.6).
    pub fn water_surface(&self, world_default: Option<f32>) -> Option<f32> {
        if !self.has_water() {
            return None;
        }
        match self.water_height {
            Some(h) if h.is_finite() && h.abs() < 1.0e30 => Some(h),
            _ if self.is_interior() => None,
            _ => world_default,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RefEntry {
    pub cell: u32,
    pub sig: [u8; 4],
    /// In a persistent-children group (type 8) rather than temporary (9) or distant (10).
    pub persistent: bool,
}

#[derive(Debug, Default)]
pub struct WorldIndex {
    pub worldspaces: BTreeMap<u32, Worldspace>,
    pub cells: HashMap<u32, Cell>,
    pub refs: HashMap<u32, RefEntry>,
    /// Children groups whose label names no CELL record in the load order.
    pub orphan_groups: Vec<u32>,
    /// Records that could not be read (FormID, message).
    pub errors: Vec<(u32, String)>,
    /// Two cells of one worldspace with the same grid square: (world, grid, earlier
    /// cell, later cell by FormID); the later one is used.
    pub grid_conflicts: Vec<(u32, (i32, i32), u32, u32)>,
    /// Optional compiled Base Object Swapper rules (`Data/BaseObjectSwapper/*.ini`).
    /// Empty unless populated via [`WorldIndex::apply_bos`].
    pub bos: crate::bos::BosTable,
    /// Whether `Data/NVSE/Plugins/hNVSE.ini` enables `bMergeRegionLists=1` so
    /// exterior `CELL` `XCLR` region lists union across load-order overrides
    /// instead of last-writer-wins [documented: hNVSE 1.2 (Nexus Mods ID 85748,
    /// `https://www.nexusmods.com/newvegas/mods/85748`) public changelog and
    /// shipped `Data\NVSE\Plugins\hNVSE.ini` comments; DLL/PDB/source not
    /// consulted]. Defaults to `false` (vanilla behavior unchanged).
    pub merge_region_lists: bool,
}

/// WRLD PNAM bits [documented: xEdit wbDefinitionsFNV.pas WRLD, schemas/fnv/world.json].
pub mod parent_use {
    pub const LAND: u16 = 1 << 0;
    pub const LOD: u16 = 1 << 1;
    pub const MAP: u16 = 1 << 2;
    pub const WATER: u16 = 1 << 3;
    pub const CLIMATE: u16 = 1 << 4;
    pub const IMAGE_SPACE: u16 = 1 << 5;
}

/// Record types in cell-children groups that are not placed references.
const NOT_REFS: [&[u8; 4]; 2] = [b"LAND", b"NAVM"];

impl WorldIndex {
    pub fn build(lo: &LoadOrder, schema: &Schema) -> WorldIndex {
        let mut ix = WorldIndex::default();
        let mut lands: HashMap<u32, u32> = HashMap::new();
        for (pi, p) in lo.plugins.iter().enumerate() {
            visit_records(&p.nodes[1..], &mut |r, stack: &[&GroupHeader]| {
                let Some(fid) = lo.to_global(pi, r.header.form_id) else { return };
                match &r.header.sig {
                    b"WRLD" => ix.add_world(lo, schema, pi, r, fid),
                    b"CELL" => ix.add_cell(lo, schema, pi, r, fid, stack),
                    sig => {
                        let Some(g) = stack.iter().rev().find(|g| {
                            matches!(
                                g.group_type,
                                group_type::CELL_PERSISTENT_CHILDREN
                                    | group_type::CELL_TEMPORARY_CHILDREN
                                    | group_type::CELL_VISIBLE_DISTANT_CHILDREN
                            )
                        }) else {
                            return;
                        };
                        let Some(cell) = lo.to_global(pi, g.label_u32()) else { return };
                        if sig == b"LAND" {
                            lands.insert(cell, fid);
                        } else if !NOT_REFS.contains(&sig) {
                            let persistent = g.group_type == group_type::CELL_PERSISTENT_CHILDREN;
                            ix.refs.insert(fid, RefEntry { cell, sig: *sig, persistent });
                        }
                    }
                }
            });
        }
        for (cell, land) in lands {
            match ix.cells.get_mut(&cell) {
                Some(c) => c.land = Some(land),
                None => ix.orphan_groups.push(cell),
            }
        }
        let mut orphans: Vec<u32> = Vec::new();
        for (&fid, e) in &ix.refs {
            match ix.cells.get_mut(&e.cell) {
                Some(c) => c.refs.push(fid),
                None => orphans.push(e.cell),
            }
        }
        ix.orphan_groups.extend(orphans);
        ix.orphan_groups.sort_unstable();
        ix.orphan_groups.dedup();
        for c in ix.cells.values_mut() {
            c.refs.sort_unstable();
        }
        // In FormID order so that a grid square claimed by two cells resolves the same way
        // every run; such conflicts are reported.
        let mut ids: Vec<u32> = ix.cells.keys().copied().collect();
        ids.sort_unstable();
        for id in ids {
            let c = &ix.cells[&id];
            if let (Some(w), Some(g)) = (c.world, c.grid) {
                let is_persistent = ix.worldspaces.get(&w).and_then(|w| w.persistent_cell) == Some(c.form_id);
                if !is_persistent {
                    if let Some(ws) = ix.worldspaces.get_mut(&w) {
                        if let Some(prev) = ws.grid.insert(g, c.form_id) {
                            ix.grid_conflicts.push((w, g, prev, c.form_id));
                        }
                    }
                }
            }
        }
        ix.take_parent_data();
        ix
    }

    /// Follow each child worldspace's parent use flags (WRLD `WNAM`, `PNAM`), once the
    /// grids are known (docs/spec-notes/world.md §2.1, #2345).
    ///
    /// What the flags mean is named, not described [documented: xEdit]. The records show
    /// it [observed: Fallout3.esm in a Tale of Two Wastelands load order, `fnvtool world
    /// list` / `heights` / `esm decode`]: a child with a flag leaves the matching
    /// subrecords out (Use Land Data: no `DNAM`; ParadiseFalls, with water, climate and
    /// image space: no `NAM2`, `CNAM`, `INAM`), a child without it has its own, and the
    /// land-sharing children lie on their parent's grid squares (MegatonWorld's own LAND
    /// equals the parent's on 10 of its 12 squares; Oasis has no LAND at all).
    ///
    /// - Use Land Data: the default heights (`DNAM`, land and water together) are the
    ///   parent's when the child has none, and every cell takes the LAND of the parent's
    ///   cell on the same grid square, its own LAND too; a square where the parent has
    ///   none keeps the child's [hypothesis, #3755: drawn, ParadiseFalls's own LAND on
    ///   (-9, 16), up to 1,550 units over the parent's, hung a ridge in the sky behind the
    ///   entrance that players of the original do not see, and GNRroofWorld's (flat at
    ///   about 12,280 over ground at about 10,700) would be a floor at roof height over
    ///   the plaza; observer check 225 asks the original].
    /// - Use Water Data: the water type and default heights are the parent's when the
    ///   child has none. Per-cell water (`XCLW`, `XCWT`) is not taken [not known].
    /// - Use Climate Data: the climate is the parent's.
    /// - Use LOD Data, Use Image Space Data: followed where they are read
    ///   ([`WorldIndex::lod_world`]; the renderer reads `INAM` with the parent's as its
    ///   fallback). Use Map Data: the Pip-Boy's map.
    ///
    /// The ten New Vegas masters' children set Use Map Data only, so nothing changes
    /// there.
    fn take_parent_data(&mut self) {
        let children: Vec<(u32, u32, u16)> = self.worldspaces.values().filter_map(|w| Some((w.form_id, w.parent?, w.parent_flags))).collect();
        for (child, parent, flags) in children {
            let Some(p) = self.worldspaces.get(&parent).cloned() else { continue };
            let land = flags & parent_use::LAND != 0;
            let water = flags & parent_use::WATER != 0;
            if let Some(w) = self.worldspaces.get_mut(&child) {
                if land || water {
                    w.default_land_height = w.default_land_height.or(p.default_land_height);
                    w.default_water_height = w.default_water_height.or(p.default_water_height);
                }
                if water {
                    w.water = w.water.or(p.water);
                }
                if flags & parent_use::CLIMATE != 0 {
                    w.climate = p.climate.or(w.climate);
                }
            }
            if land {
                let squares: Vec<((i32, i32), u32)> = self.worldspaces[&child].grid.iter().map(|(&g, &c)| (g, c)).collect();
                for (g, cell) in squares {
                    let Some(theirs) = p.grid.get(&g).and_then(|c| self.cells.get(c)).and_then(|c| c.land) else { continue };
                    if let Some(c) = self.cells.get_mut(&cell) {
                        c.land = Some(theirs);
                        c.land_of_parent = true;
                    }
                }
            }
        }
    }

    /// The worldspace whose distant LOD files `world` shows: its parent with Use LOD Data
    /// [documented name: xEdit; hypothesis: the parent's files are the ones drawn], else
    /// itself.
    pub fn lod_world(&self, world: u32) -> u32 {
        self.worldspaces.get(&world).filter(|w| w.parent_flags & parent_use::LOD != 0).and_then(|w| w.parent).filter(|p| self.worldspaces.contains_key(p)).unwrap_or(world)
    }

    fn add_world(&mut self, lo: &LoadOrder, schema: &Schema, pi: usize, r: &fnv_esm::Record, fid: u32) {
        let f = match Fields::new(r, pi, schema) {
            Ok(f) => f,
            Err(e) => return self.errors.push((fid, e.to_string())),
        };
        let w = self.worldspaces.entry(fid).or_default();
        w.form_id = fid;
        w.edid = f.string(b"EDID");
        w.name = f.string(b"FULL");
        w.parent = f.formid(b"WNAM", lo);
        w.climate = f.formid(b"CNAM", lo);
        w.water = f.formid(b"NAM2", lo);
        w.encounter_zone = f.formid(b"XEZN", lo);
        w.parent_flags = f.get(b"PNAM").and_then(|v| num(&v, "value")).map_or(0, |v| v as u16);
        w.data_flags = f.get(b"DATA").and_then(|v| num(&v, "value")).map_or(0, |v| v as u8);
        if let Some(d) = f.get(b"DNAM") {
            w.default_land_height = num(&d, "default_land_height").map(|v| v as f32);
            w.default_water_height = num(&d, "default_water_height").map(|v| v as f32);
        }
    }

    fn add_cell(
        &mut self,
        lo: &LoadOrder,
        schema: &Schema,
        pi: usize,
        r: &fnv_esm::Record,
        fid: u32,
        stack: &[&GroupHeader],
    ) {
        let f = match Fields::new(r, pi, schema) {
            Ok(f) => f,
            Err(e) => return self.errors.push((fid, e.to_string())),
        };
        let world_group = stack.iter().rev().find(|g| g.group_type == group_type::WORLD_CHILDREN);
        let world = world_group.and_then(|g| lo.to_global(pi, g.label_u32()));
        let c = self.cells.entry(fid).or_default();
        c.form_id = fid;
        c.edid = f.string(b"EDID");
        c.name = f.string(b"FULL");
        c.world = world;
        c.plugin = pi;
        c.data_flags = f.get(b"DATA").and_then(|v| num(&v, "value")).map_or(0, |v| v as u8);
        c.grid = f.get(b"XCLC").and_then(|v| Some((num(&v, "x")? as i32, num(&v, "y")? as i32)));
        c.water_height = f.get(b"XCLW").and_then(|v| v.as_f64()).map(|v| v as f32);
        c.water_type = f.formid(b"XCWT", lo);
        c.encounter_zone = f.formid(b"XEZN", lo);
        // A cell directly inside the world-children group (no block group after it) is the
        // worldspace's persistent cell [observed: one per worldspace, see world.md].
        if let (Some(w), Some(last)) = (world, stack.last()) {
            if last.group_type == group_type::WORLD_CHILDREN {
                self.worldspaces.entry(w).or_default().persistent_cell = Some(fid);
            }
        }
    }

    pub fn world_by_name(&self, name: &str) -> Option<&Worldspace> {
        if let Some(fid) = formid_of(&serde_json::Value::String(name.to_string())) {
            if let Some(w) = self.worldspaces.get(&fid) {
                return Some(w);
            }
        }
        self.worldspaces.values().find(|w| w.edid.as_deref().is_some_and(|e| e.eq_ignore_ascii_case(name)))
    }

    pub fn cell_by_name(&self, name: &str) -> Option<&Cell> {
        if let Some(fid) = formid_of(&serde_json::Value::String(name.to_string())) {
            if let Some(c) = self.cells.get(&fid) {
                return Some(c);
            }
        }
        self.cells.values().find(|c| c.edid.as_deref().is_some_and(|e| e.eq_ignore_ascii_case(name)))
    }

    /// Attach a compiled [`crate::bos::BosTable`] so placement consumers sharing this
    /// index resolve swapped base objects consistently.
    pub fn apply_bos(&mut self, bos: crate::bos::BosTable) {
        self.bos = bos;
    }

    /// Enable or disable `hNVSE.ini` `bMergeRegionLists` exterior `CELL` `XCLR`
    /// region-list merging across load-order overrides.
    pub fn set_merge_region_lists(&mut self, enabled: bool) {
        self.merge_region_lists = enabled;
    }

    /// Resolve the effective base FormID for placed reference `ref_id` with original
    /// `orig_base`, applying [`Self::bos`] when non-empty.
    pub fn resolve_base(&self, ref_id: u32, orig_base: u32) -> u32 {
        self.bos.resolve(ref_id, orig_base)
    }

    /// Winning placement of reference `fid` with [`Self::bos`] applied to `base`.
    pub fn placement(&self, lo: &LoadOrder, schema: &Schema, fid: u32) -> Option<crate::placed::Placement> {
        crate::placed::placement_with_bos(lo, schema, &self.bos, fid)
    }
}

/// Convenience for callers that hold a plugin-local FormID value from a decoded field.
pub fn global_of(lo: &LoadOrder, plugin: usize, v: &serde_json::Value) -> Option<u32> {
    global(lo, plugin, formid_of(v)?)
}
