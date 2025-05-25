//! This is a replacement for CellFmtArray for use within
//! Operation::SetFormatSelection, and eventually to replace the Format db for
//! the sheet.
//!
//! Formats is used to store multiple formats for use in Operations.

mod format;
mod format_update;
mod sheet_format_updates;

use crate::RunLengthEncoding;
use serde::{Deserialize, Serialize};
use std::ops::{Deref, DerefMut};

pub use format::Format;
pub use format_update::FormatUpdate;
pub use sheet_format_updates::{SheetFormatUpdates, SheetFormatUpdatesType};

/// Run-length encoded changes to apply to formatting.
#[derive(Default, Serialize, Deserialize, Debug, Clone, Eq, PartialEq)]
pub struct Formats {
    pub formats: RunLengthEncoding<FormatUpdate>,
}

impl Formats {
    pub fn new() -> Self {
        Self {
            formats: RunLengthEncoding::new(),
        }
    }

    pub fn repeat(update: FormatUpdate, count: usize) -> Self {
        let mut formats = Formats::default();
        formats.push_n(update, count);
        formats
    }
}

impl Deref for Formats {
    type Target = RunLengthEncoding<FormatUpdate>;

    fn deref(&self) -> &Self::Target {
        &self.formats
    }
}

impl DerefMut for Formats {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.formats
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeat() {
        let update = FormatUpdate::default();
        let formats = Formats::repeat(update.clone(), 3);
        assert_eq!(formats.size(), 3);
        assert_eq!(formats.get_at(0), Some(&update));
        assert_eq!(formats.get_at(1), Some(&update));
        assert_eq!(formats.get_at(2), Some(&update));
    }
}
