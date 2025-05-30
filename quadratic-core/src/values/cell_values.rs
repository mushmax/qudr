//! CellValues is a 2D array of CellValue used for Operation::SetCellValues.
//! The width and height may grow as needed.

use crate::{Array, ArraySize, CellValue, Rect};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Sparsely-populated rectangle of [`CellValue`]s.
#[derive(Default, Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CellValues {
    pub columns: Vec<BTreeMap<u64, CellValue>>,
    pub w: u32,
    pub h: u32,
}

impl CellValues {
    pub fn new(w: u32, h: u32) -> Self {
        Self {
            columns: vec![BTreeMap::new(); w as usize],
            w,
            h,
        }
    }

    pub fn new_blank(w: u32, h: u32) -> Self {
        let mut columns = Vec::with_capacity(w as usize);
        let mut column = BTreeMap::new();
        for y in 0..h {
            column.insert(y as u64, CellValue::Blank);
        }
        for _ in 0..w {
            columns.push(column.clone());
        }
        Self { columns, w, h }
    }

    pub fn get_except_blank(&self, x: u32, y: u32) -> Option<&CellValue> {
        assert!(x < self.w && y < self.h, "CellValues::get out of bounds");
        self.columns
            .get(x as usize)
            .and_then(|col| col.get(&(y as u64)))
            .and_then(|value| {
                if value == &CellValue::Blank {
                    None
                } else {
                    Some(value)
                }
            })
    }

    pub fn get(&self, x: u32, y: u32) -> Option<&CellValue> {
        assert!(x < self.w && y < self.h, "CellValues::get out of bounds");
        self.columns
            .get(x as usize)
            .and_then(|col| col.get(&(y as u64)))
    }

    pub fn safe_get(&self, x: u32, y: u32) -> anyhow::Result<&CellValue> {
        if !(x < self.w && y < self.h) {
            anyhow::bail!(
                "CellValues::safe_get out of bounds: w={}, h={}, x={}, y={}",
                self.w,
                self.h,
                x,
                y
            );
        }

        let cell_value = self
            .columns
            .get(x as usize)
            .and_then(|col| col.get(&(y as u64)))
            .ok_or_else(|| anyhow::anyhow!("No value found at ({x}, {y})"))?;

        Ok(cell_value)
    }

    pub fn get_owned(&mut self, x: u32, y: u32) -> anyhow::Result<&mut CellValue> {
        if !(x < self.w && y < self.h) {
            anyhow::bail!(
                "CellValues::safe_get out of bounds: w={}, h={}, x={}, y={}",
                self.w,
                self.h,
                x,
                y
            );
        }

        let column = self
            .columns
            .get_mut(x as usize)
            .ok_or_else(|| anyhow::anyhow!("No column found at {x}"))?;

        column
            .get_mut(&(y as u64))
            .ok_or_else(|| anyhow::anyhow!("No value found at ({x}, {y})"))
    }

    pub fn get_rect(&mut self, rect: Rect) -> Vec<Vec<CellValue>> {
        let mut values =
            vec![vec![CellValue::Blank; rect.width() as usize]; rect.height() as usize];

        for (y_index, y) in rect.y_range().enumerate() {
            for (x_index, x) in rect.x_range().enumerate() {
                let new_x = u32::try_from(x).unwrap_or(0);
                let new_y = u32::try_from(y).unwrap_or(0);

                values[y_index][x_index] = self.remove(new_x, new_y).unwrap_or(CellValue::Blank);
            }
        }
        values
    }

    pub fn set(&mut self, x: u32, y: u32, value: CellValue) {
        if y >= self.h {
            self.h = y + 1;
        }

        // w can grow if too small
        if x >= self.w {
            for _ in self.w..=x {
                self.columns.push(BTreeMap::new());
                self.w += 1;
            }
        }
        self.columns[x as usize].insert(y as u64, value);
    }

    pub fn remove(&mut self, x: u32, y: u32) -> Option<CellValue> {
        assert!(
            x < self.w && y < self.h,
            "CellValues::remove out of bounds: x={x}, y={y}, w={}, h={}",
            self.w,
            self.h
        );
        self.columns[x as usize].remove(&(y as u64))
    }

    pub fn size(&self) -> u32 {
        self.w * self.h
    }

    pub fn is_empty(&self) -> bool {
        self.size() == 0
    }

    /// Creates CellValues from a flat array of CellValue given a width and height
    pub fn from_flat_array(w: u32, h: u32, values: Vec<CellValue>) -> Self {
        assert!(
            w * h == values.len() as u32,
            "CellValues::flat_array size mismatch, expected {}, got {}",
            w * h,
            values.len()
        );
        let mut columns = vec![BTreeMap::new(); w as usize];
        for (i, value) in values.into_iter().enumerate() {
            let x = (i as u32) % w;
            let y = (i as u32) / w;
            columns[x as usize].insert(y as u64, value);
        }
        Self { columns, w, h }
    }

    pub fn into_iter(&self) -> impl Iterator<Item = (u32, u32, &CellValue)> {
        self.columns.iter().enumerate().flat_map(|(x, col)| {
            col.iter()
                .map(move |(y, value)| (x as u32, *y as u32, value))
        })
    }

    pub fn into_owned_iter(self) -> impl Iterator<Item = (u32, u32, CellValue)> {
        self.columns.into_iter().enumerate().flat_map(|(x, col)| {
            col.into_iter()
                .map(move |(y, value)| (x as u32, y as u32, value))
        })
    }

    pub fn into_vec(&mut self) -> Vec<Vec<CellValue>> {
        let width = self.w as i64;
        let height = self.h as i64;

        self.get_rect(Rect::new(0, 0, width, height))
    }

    pub fn into_owned_vec(self) -> Vec<Vec<CellValue>> {
        let mut vec = vec![vec![CellValue::Blank; self.w as usize]; self.h as usize];
        for (x, col) in self.columns.into_iter().enumerate() {
            for (y, value) in col.into_iter() {
                vec[y as usize][x] = value;
            }
        }
        vec
    }

    #[cfg(test)]
    /// Creates a CellValues from a CellValue, including CellValue::Blank (which is ignored in into)
    pub fn from_cell_value(value: CellValue) -> Self {
        let mut c = Self::new(1, 1);
        c.set(0, 0, value);
        c
    }

    #[cfg(test)]
    /// Creates a CellValues from a 2D array of CellValue, including
    /// CellValue::Blank (which is ignored in into)
    pub fn from_cell_value_vec(values: Vec<Vec<CellValue>>) -> Self {
        let w = values.iter().map(|col| col.len() as u32).max().unwrap_or(0);
        let h = values.len() as u32;
        let mut columns = vec![BTreeMap::new(); w as usize];
        for (y, col) in values.into_iter().enumerate() {
            for (x, value) in col.into_iter().enumerate() {
                columns[x].insert(y as u64, value);
            }
        }
        Self { columns, w, h }
    }
}

/// Converts a 2D array of CellValue into CellValues
/// The first dimension is the y-axis, the second is the x-axis.
/// Therefore, [[1, 2, 3], [4, 5, 6]] becomes:
/// 1 4
/// 2 5
/// 3 6
impl From<Vec<Vec<CellValue>>> for CellValues {
    fn from(values: Vec<Vec<CellValue>>) -> Self {
        let w = values.iter().map(|col| col.len() as u32).max().unwrap_or(0);
        let h = values.len() as u32;
        let mut columns = vec![BTreeMap::new(); w as usize];
        for (y, col) in values.into_iter().enumerate() {
            for (x, value) in col.into_iter().enumerate() {
                columns[x].insert(y as u64, value);
            }
        }
        Self { columns, w, h }
    }
}

/// Converts a sparse 2D array of Vec<Vec<Option<CellValue>>> into CellValues
/// The first dimension is the x-axis, the second is the y-axis.
/// This is a different format the the `Vec<Vec<CellValue>>` impl above.
impl From<Vec<Vec<Option<CellValue>>>> for CellValues {
    fn from(values: Vec<Vec<Option<CellValue>>>) -> Self {
        let w = values.len() as u32;
        let h = values[0].len() as u32;
        let mut cell_values = CellValues::new(w, h);

        for (x, col) in values.into_iter().enumerate() {
            for (y, value) in col.into_iter().enumerate() {
                if let Some(value) = value {
                    cell_values.set(x as u32, y as u32, value);
                }
            }
        }

        cell_values
    }
}

/// Convert a 2D array of strings into a CellValues.
/// The first dimension is the x-axis, the second is the y-axis.
/// Therefore, [[1, 2, 3], [4, 5, 6]] becomes:
/// 1 2 3
/// 4 5 6
impl From<Vec<Vec<&str>>> for CellValues {
    fn from(values: Vec<Vec<&str>>) -> Self {
        let w = values.len() as u32;
        let h = values.iter().map(|col| col.len() as u32).max().unwrap_or(0);
        let mut columns = vec![BTreeMap::new(); w as usize];
        for (x, col) in values.into_iter().enumerate() {
            for (y, value) in col.into_iter().enumerate() {
                if !value.is_empty() {
                    columns[x].insert(y as u64, CellValue::from(value));
                }
            }
        }
        Self { columns, w, h }
    }
}

impl From<CellValue> for CellValues {
    fn from(value: CellValue) -> Self {
        let mut c = Self::new(1, 1);
        c.set(0, 0, value);
        c
    }
}

impl From<Array> for CellValues {
    fn from(array: Array) -> Self {
        let ArraySize { w, h } = array.size();
        let cell_values_vec = array.into_cell_values_vec().into_vec();

        CellValues::from_flat_array(w.get(), h.get(), cell_values_vec)
    }
}

impl From<CellValues> for Vec<Vec<CellValue>> {
    fn from(mut cell_values: CellValues) -> Self {
        cell_values.into_vec()
    }
}

#[cfg(test)]
mod test {

    use crate::wasm_bindings::js::clear_js_calls;

    use super::*;

    #[test]
    fn new() {
        let cell_values = CellValues::new(2, 3);
        assert_eq!(cell_values.w, 2);
        assert_eq!(cell_values.h, 3);
        assert_eq!(cell_values.columns.len(), 2);
        assert_eq!(cell_values.columns[0].len(), 0);
        assert_eq!(cell_values.columns[1].len(), 0);
    }

    #[test]
    fn new_blank() {
        let cell_values = CellValues::new_blank(2, 3);
        assert_eq!(cell_values.w, 2);
        assert_eq!(cell_values.h, 3);
        assert_eq!(cell_values.columns.len(), 2);
        assert_eq!(cell_values.columns[0].len(), 3);
        assert_eq!(cell_values.columns[1].len(), 3);
        assert_eq!(cell_values.columns[0].get(&0), Some(&CellValue::Blank));
        assert_eq!(cell_values.columns[1].get(&0), Some(&CellValue::Blank));
        assert_eq!(cell_values.columns[0].get(&1), Some(&CellValue::Blank));
        assert_eq!(cell_values.columns[1].get(&1), Some(&CellValue::Blank));
        assert_eq!(cell_values.columns[0].get(&2), Some(&CellValue::Blank));
        assert_eq!(cell_values.columns[1].get(&2), Some(&CellValue::Blank));
    }

    #[test]
    fn get_set_remove() {
        let mut cell_values = CellValues::new(2, 3);
        cell_values.set(0, 0, CellValue::from("a"));
        cell_values.set(1, 2, CellValue::from("b"));
        assert_eq!(cell_values.get(0, 0), Some(&CellValue::from("a")));
        assert_eq!(cell_values.get(1, 2), Some(&CellValue::from("b")));
        assert_eq!(cell_values.get(1, 0), None);
        cell_values.remove(0, 0);
        assert_eq!(cell_values.get(0, 0), None);
    }

    #[test]
    fn get_except_blank() {
        let mut cell_values = CellValues::new(2, 3);
        cell_values.set(0, 0, CellValue::from("a"));
        cell_values.set(1, 2, CellValue::Blank);
        assert_eq!(
            cell_values.get_except_blank(0, 0),
            Some(&CellValue::from("a"))
        );
        assert_eq!(cell_values.get_except_blank(1, 2), None);
    }

    #[test]
    fn from_str() {
        let cell_values = CellValues::from(vec![vec!["a", "b"], vec!["c", "d"]]);
        assert_eq!(cell_values.w, 2);
        assert_eq!(cell_values.h, 2);
        assert_eq!(cell_values.get(0, 0), Some(&CellValue::from("a")));
        assert_eq!(cell_values.get(1, 1), Some(&CellValue::from("d")));
    }

    #[test]
    fn size() {
        let cell_values = CellValues::new(2, 3);
        assert_eq!(cell_values.size(), 6);
    }

    #[test]
    fn from_cell_value() {
        let cell_values =
            CellValues::from(vec![vec![CellValue::from("a")], vec![CellValue::from("b")]]);
        assert_eq!(cell_values.w, 1);
        assert_eq!(cell_values.h, 2);
        assert_eq!(cell_values.get(0, 0), Some(&CellValue::from("a")));
        assert_eq!(cell_values.get(0, 1), Some(&CellValue::from("b")));
    }

    #[test]
    fn from_cell_value_single() {
        let cell_values = CellValues::from(CellValue::from("a"));
        assert_eq!(cell_values.w, 1);
        assert_eq!(cell_values.h, 1);
        assert_eq!(cell_values.get(0, 0), Some(&CellValue::from("a")));
    }

    #[test]
    fn from_flat_array() {
        let cell_values = CellValues::from_flat_array(2, 3, vec![CellValue::from("a"); 6]);
        assert_eq!(cell_values.w, 2);
        assert_eq!(cell_values.h, 3);
        assert_eq!(cell_values.get(0, 0), Some(&CellValue::from("a")));
        assert_eq!(cell_values.get(1, 2), Some(&CellValue::from("a")));
    }

    #[test]
    fn into_iter() {
        let cell_values = CellValues::from(vec![vec!["a", "b"], vec!["c", "d"]]);
        let mut iter = cell_values.into_iter();
        assert_eq!(iter.next(), Some((0, 0, &CellValue::from("a"))));
        assert_eq!(iter.next(), Some((0, 1, &CellValue::from("b"))));
        assert_eq!(iter.next(), Some((1, 0, &CellValue::from("c"))));
        assert_eq!(iter.next(), Some((1, 1, &CellValue::from("d"))));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn cell_values_serialize_large() {
        let w = 100;
        let h = 10000;
        let mut cell_values = CellValues::new(w, h);
        for x in 0..w {
            for y in 0..h {
                cell_values.set(x, y, CellValue::from("a"));
            }
        }
        let json = serde_json::to_string(&cell_values).unwrap();
        assert!(json.len() > (w * h * 3) as usize);
        clear_js_calls();
    }

    #[test]
    fn cell_values_w_grows() {
        let mut cell_values = CellValues::new(1, 1);
        cell_values.set(1, 0, CellValue::from("a"));
        assert_eq!(cell_values.w, 2);
        assert_eq!(cell_values.columns.len(), 2);
        assert_eq!(cell_values.h, 1);
        assert_eq!(cell_values.get(1, 0), Some(&CellValue::from("a")));
    }

    #[test]
    fn cell_values_from_vec_of_vec_of_option() {
        let mut cell_values = vec![vec![None; 1]; 4];
        cell_values[0][0] = Some(CellValue::from("a"));
        cell_values[1][0] = Some(CellValue::from("b"));
        cell_values[3][0] = Some(CellValue::from("c"));
        let cell_values = CellValues::from(cell_values);

        assert_eq!(cell_values.w, 4);
        assert_eq!(cell_values.h, 1);
        assert_eq!(cell_values.get(0, 0), Some(&CellValue::from("a")));
        assert_eq!(cell_values.get(1, 0), Some(&CellValue::from("b")));
        assert_eq!(cell_values.get(3, 0), Some(&CellValue::from("c")));
    }
}
