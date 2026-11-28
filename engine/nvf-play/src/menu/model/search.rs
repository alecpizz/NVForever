//! Transient item-menu filtering (native-menu-search.md); input/publication is separate.
use super::{FolderRow, Listing, MenuModel, Row, RowKey, Side};
use fnv_sim::{world::SimError, World};

/// Mapped input budget, not an original-game limit.
pub const MAX_SEARCH_CHARS: usize = 256;

#[derive(Clone, Debug, Default)]
pub(super) struct Search {
    text: String,
    needle: String,
    hidden: [usize; 2],
    changed: bool,
}

#[derive(Debug)]
pub enum SearchError {
    TooLong,
    QuantityOpen,
    Simulation(SimError),
}

impl std::fmt::Display for SearchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLong => write!(f, "search exceeds {MAX_SEARCH_CHARS} characters"),
            Self::QuantityOpen => f.write_str("close the quantity prompt before changing search"),
            Self::Simulation(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for SearchError {}

#[derive(Clone, Copy)]
pub(super) struct Selection {
    key: RowKey,
    bartered: bool,
    folder: Option<FolderRow>,
    live: bool,
}

impl Search {
    pub(super) fn capture(&self, model: &MenuModel, side: Side) -> Option<Selection> {
        if !self.changed && self.needle.is_empty() {
            return None;
        }
        let index = model.selected[side as usize];
        let rows = &model.list(side).rows;
        let row = rows.get(index)?;
        Some(Selection {
            key: RowKey::at(rows, index)?,
            bartered: row.bartered,
            folder: row.folder,
            live: row.live,
        })
    }

    pub(super) fn filter(&mut self, lists: &mut [Listing; 2], sides: &[Side]) {
        self.hidden = [0; 2];
        if !self.needle.is_empty() {
            for &side in sides {
                let rows = &mut lists[side as usize].rows;
                let before = rows.len();
                rows.retain(|row| row.folder == Some(FolderRow::Back) || row.name.to_lowercase().contains(&self.needle));
                self.hidden[side as usize] = before - rows.len();
            }
        }
        self.changed = false;
    }
}

impl Selection {
    pub(super) fn find(self, rows: &[Row]) -> Option<usize> {
        if matches!(self.folder, Some(FolderRow::Open(super::Folder::Ysi(_)))) {
            return rows.iter().position(|row| row.folder == self.folder);
        }
        let mut nth = 0;
        rows.iter().position(|row| {
            if row.units() != self.key.units
                || row.reference != self.key.reference
                || matches!(row.folder, Some(FolderRow::Open(super::Folder::Ysi(_))))
            {
                return false;
            }
            let at = nth;
            nth += 1;
            at == self.key.nth && row.bartered == self.bartered && row.folder == self.folder && row.live == self.live
        })
    }
}

impl MenuModel {
    pub fn search_text(&self) -> &str {
        &self.search.text
    }

    /// Additional rows hidden by search, separate from existing category counts.
    pub fn search_hidden(&self, side: Side) -> usize {
        self.search.hidden[side as usize]
    }

    /// Rebuild through the existing inventory/barter consumer, then filter shown rows.
    pub fn set_search(&mut self, world: &mut World<'_>, text: &str) -> Result<bool, SearchError> {
        if text.chars().count() > MAX_SEARCH_CHARS {
            return Err(SearchError::TooLong);
        }
        if self.search.text == text {
            return Ok(false);
        }
        if self.quantity.is_some() {
            return Err(SearchError::QuantityOpen);
        }
        self.search.text = text.to_string();
        self.search.needle = text.to_lowercase();
        self.search.changed = true;
        self.pointer_pick = false;
        self.refresh(world).map_err(SearchError::Simulation)?;
        Ok(true)
    }
}
