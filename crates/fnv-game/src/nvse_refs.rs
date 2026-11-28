//! xNVSE's reference and cell functions (docs/handoff/xnvse-remaining.md §4, family 1):
//! walking the references of the loaded cells (GetFirstRef, GetNextRef, GetNumRefs, the
//! …InCell forms, GetRefs, GetRefsInCell) and reading a reference (GetParentWorldspace,
//! IsPersistent, GetOwner) [documented: their pages].

use crate::game::Game;
use fnv_script::vm::{ArrayKind, XKey, XVal};
use fnv_sim::FormId;

/// The base-form types GetFirstRef's code 201 ("inventory items") covers [hypothesis:
/// the records a container can hold].
const ITEM_SIGS: [&[u8; 4]; 12] = [b"WEAP", b"ARMO", b"AMMO", b"MISC", b"ALCH", b"BOOK", b"KEYM", b"NOTE", b"IMOD", b"CMNY", b"CCRD", b"CHIP"];

impl Game<'_> {
    /// The references now in `cell`, in FormID order: placed there or moved there. One
    /// picked up (deleted) only with `taken` [documented: GetFirstRef, IncludeTakenRefs].
    pub(crate) fn refs_in_cell(&self, cell: FormId, taken: bool) -> Vec<FormId> {
        let mut out: Vec<FormId> = self.world.placed.by_cell.get(&cell).into_iter().flatten().copied().collect();
        out.extend(self.world.changed().filter(|(_, s)| s.cell == Some(cell)).map(|(r, _)| r));
        out.sort();
        out.dedup();
        out.retain(|&r| self.cell_of(r) == Some(cell) && (taken || !self.world.peek(r).is_some_and(|s| s.deleted())));
        out
    }

    /// The cells GetFirstRef scans: the cell, and in an exterior the cells within
    /// `depth` grid squares of it (depth 1: 9 cells, 2: 25) [documented].
    pub(crate) fn scan_cells(&self, cell: FormId, depth: i64) -> Vec<FormId> {
        let Some(world) = self.world.placed.world_of(cell) else { return vec![cell] };
        let Some(grid) = self.world.placed.world_grid.get(&world) else { return vec![cell] };
        let Some(&(x, y)) = grid.iter().find(|(_, &c)| c == cell).map(|(g, _)| g) else { return vec![cell] };
        let d = depth.clamp(0, 8) as i32;
        let mut out = vec![cell];
        for gx in x - d..=x + d {
            for gy in y - d..=y + d {
                if let Some(&c) = grid.get(&(gx, gy)) {
                    if c != cell {
                        out.push(c);
                    }
                }
            }
        }
        out
    }

    /// Whether a reference is of GetFirstRef's type `code`: 0 any, 200 actors, 201
    /// inventory items, else the base form's form type code [documented: GetFirstRef,
    /// Form Type IDs].
    pub(crate) fn ref_of_type(&self, r: FormId, code: i64) -> bool {
        if code == 0 {
            return true;
        }
        let base = crate::functions::base_of(self, r);
        let sig = base.and_then(|b| self.data.sig(b));
        match code {
            200 => matches!(sig.as_ref(), Some(b"NPC_" | b"CREA")) || r == FormId::PLAYER_REF,
            201 => sig.is_some_and(|s| ITEM_SIGS.contains(&&s)),
            c => sig.and_then(|s| fnv_script::vm::form_type_code(&s)).is_some_and(|t| i64::from(t) == c),
        }
    }

    /// The references a walk covers. In a cell that is not loaded only the persistent
    /// ones [documented: GetFirstRefInCell; observed: nvse-refs, an unloaded house counts
    /// its persistent references only]. Loaded: the player's cell and, outside, the cells
    /// within two squares of it [hypothesis: uGridsToLoad 5].
    pub(crate) fn ref_walk_list(&self, cell: Option<FormId>, code: i64, depth: i64, taken: bool) -> Vec<FormId> {
        let here = self.cell_of(FormId::PLAYER_REF);
        let Some(cell) = cell.or(here) else { return Vec::new() };
        let loaded: Vec<FormId> = here.map(|h| self.scan_cells(h, 2)).unwrap_or_default();
        let cells = if depth > 0 { self.scan_cells(cell, depth) } else { vec![cell] };
        if cells.len() == 1 {
            let c = cells[0];
            let open = loaded.contains(&c);
            return self
                .refs_in_cell(c, taken)
                .into_iter()
                .filter(|&r| (open || self.persistent(r)) && r != FormId::PLAYER_REF && self.ref_of_type(r, code))
                .collect();
        }
        // Multi-cell walk: scan `world.changed()` once across all target cells instead of
        // repeating a full `World::refs` scan for every cell in the grid (`depth = 2` is 25 cells).
        let cell_set: std::collections::HashSet<FormId> = cells.iter().copied().collect();
        let mut moved_by_cell: std::collections::HashMap<FormId, Vec<FormId>> = std::collections::HashMap::new();
        for (r, s) in self.world.changed() {
            if let Some(c) = s.cell.filter(|c| cell_set.contains(c)) {
                moved_by_cell.entry(c).or_default().push(r);
            }
        }
        let mut out = Vec::new();
        for c in cells {
            let open = loaded.contains(&c);
            let mut in_cell: Vec<FormId> = self.world.placed.by_cell.get(&c).into_iter().flatten().copied().collect();
            if let Some(extra) = moved_by_cell.remove(&c) {
                in_cell.extend(extra);
            }
            in_cell.sort();
            in_cell.dedup();
            in_cell.retain(|&r| self.cell_of(r) == Some(c) && (taken || !self.world.peek(r).is_some_and(|s| s.deleted())));
            for r in in_cell {
                if (open || self.persistent(r)) && r != FormId::PLAYER_REF && self.ref_of_type(r, code) {
                    out.push(r);
                }
            }
        }
        out
    }

    pub(crate) fn persistent(&self, r: FormId) -> bool {
        if let Some(&on) = self.other.persistent_overrides.get(&r) {
            return on;
        }
        if r == FormId::PLAYER_REF {
            return true;
        }
        self.world.placed.get(r).is_some_and(|p| p.flags & fnv_sim::refs::FLAG_PERSISTENT != 0 || p.in_persistent_group)
    }
}

fn int(args: &[XVal], k: usize, dflt: i64) -> i64 {
    match args.get(k) {
        Some(XVal::Num(x)) => *x as i64,
        _ => dflt,
    }
}

fn form(args: &[XVal], k: usize) -> Option<FormId> {
    match args.get(k)? {
        XVal::Ref(0) => None,
        XVal::Ref(r) => Some(FormId(*r)),
        _ => None,
    }
}

/// The reference and cell functions; `None` for any other name.
pub(crate) fn call(g: &mut Game, name: &str, subject: Option<FormId>, args: &[XVal]) -> Option<Result<XVal, String>> {
    let v = match name {
        // `[cell] type depth taken`.
        "getfirstref" | "getfirstrefincell" | "getnumrefs" | "getnumrefsincell" | "getrefs" | "getrefsincell" => {
            let in_cell = name.ends_with("incell");
            let (cell, k) = if in_cell { (form(args, 0), 1) } else { (None, 0) };
            let list = g.ref_walk_list(cell, int(args, k, 0), int(args, k + 1, 0), int(args, k + 2, 0) != 0);
            if name.starts_with("getnumrefs") {
                XVal::Num(list.len() as f64)
            } else if name.starts_with("getrefs") {
                // `maxDistance` leaves out the references farther than it [documented:
                // GetRefs, GetRefsInCell], measured from the calling reference, else the
                // player [hypothesis: as CallPerRef's distance, documented]. A sixth
                // argument compiles (script-compiler.md §8.1) and is not read: what it
                // does is not known.
                let max = match args.get(k + 3) {
                    Some(XVal::Num(x)) => *x,
                    _ => 0.0,
                };
                let me = subject.unwrap_or(FormId::PLAYER_REF);
                let list: Vec<FormId> = if max > 0.0 { list.into_iter().filter(|&r| crate::nvse_showoff5::distance_between(g, me, r).is_some_and(|d| d <= max)).collect() } else { list };
                let entries = list.iter().enumerate().map(|(i, r)| (XKey::Num(i as f64), XVal::Ref(r.0))).collect();
                XVal::Array(g.ext.new_array(ArrayKind::List, entries))
            } else {
                let first = list.first().map_or(0, |r| r.0);
                g.ref_walk = list.into_iter().skip(1).collect();
                XVal::Ref(first)
            }
        }
        // [documented: GetNextRef] 0 when none are left.
        "getnextref" => {
            if g.ref_walk.is_empty() {
                XVal::Ref(0)
            } else {
                XVal::Ref(g.ref_walk.remove(0).0)
            }
        }
        "getparentworldspace" => XVal::Ref(subject.and_then(|r| g.world.worldspace_of(r)).map_or(0, |w| w.0)),
        // Persistent: the record's flag (0x400) or a cell's persistent group [documented:
        // IsPersistent; the flag bit, xEdit REFR record flags].
        "ispersistent" => {
            XVal::Num(f64::from(u8::from(subject.is_some_and(|r| g.persistent(r)))))
        }
        // [documented: GetOwner] the reference's own owner (live extra.owner from
        // SetOwnership/ClearOwnership/SetOwnershipTemp, else the placed record's XOWN): a
        // faction or an NPC; for an inventory reference, its stack's [observed: nvse-fifth,
        // AddItemOwnership's pistol gives Doc].
        "getowner" => {
            let stack = subject.and_then(|r| g.temp_item(r)).and_then(|(c, it)| g.world.inventory_mut(c).ok()?.groups().find(|(i, _, _)| *i == it).and_then(|(_, e, _)| e.owner));
            XVal::Ref(stack.or_else(|| subject.and_then(|r| Game::ref_owner_in(&g.world, r))).map_or(0, |o| o.0))
        }
        _ => return None,
    };
    Some(Ok(v))
}
