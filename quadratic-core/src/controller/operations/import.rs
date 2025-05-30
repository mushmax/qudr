use std::{borrow::Cow, io::Cursor};

use anyhow::{Result, anyhow, bail};
use chrono::{NaiveDate, NaiveTime};
use csv_sniffer::Sniffer;

use crate::{
    Array, ArraySize, CellValue, Pos, SheetPos,
    arrow::arrow_col_to_cell_value_vec,
    cellvalue::Import,
    controller::GridController,
    grid::{
        CodeCellLanguage, CodeCellValue, DataTable, Sheet, SheetId,
        file::sheet_schema::export_sheet, formats::SheetFormatUpdates,
    },
};
use bytes::Bytes;
use calamine::{Data as ExcelData, Reader as ExcelReader, Xlsx, XlsxError};
use lexicon_fractional_index::key_between;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;

use super::operation::Operation;

const IMPORT_LINES_PER_OPERATION: u32 = 10000;

impl GridController {
    /// Guesses if the first row of a CSV file is a header based on the types of the
    /// first three rows.
    pub fn guess_csv_first_row_is_header(&self, cell_values: &Array) -> bool {
        if cell_values.height() < 3 {
            return false;
        }

        let types = |row: usize| {
            cell_values
                .get_row(row)
                .unwrap_or_default()
                .iter()
                .map(|c| c.type_id())
                .collect::<Vec<_>>()
        };

        let row_0 = types(0);
        let row_1 = types(1);
        let row_2 = types(2);

        let row_0_is_different_from_row_1 = row_0 != row_1;
        let row_1_is_same_as_row_2 = row_1 == row_2;

        row_0_is_different_from_row_1 && row_1_is_same_as_row_2
    }

    pub fn get_csv_preview(
        file: Vec<u8>,
        max_rows: u32,
        delimiter: Option<u8>,
    ) -> Result<Vec<Vec<String>>> {
        let error = |message: String| anyhow!("Error parsing CSV file for preview: {}", message);
        let file: &[u8] = match String::from_utf8_lossy(&file) {
            std::borrow::Cow::Borrowed(_) => &file,
            std::borrow::Cow::Owned(_) => {
                if let Some(utf) = read_utf16(&file) {
                    return Self::get_csv_preview(utf.as_bytes().to_vec(), max_rows, delimiter);
                }
                &file
            }
        };

        let delimiter = match delimiter {
            Some(d) => d,
            None => {
                // auto detect the delimiter, default to ',' if it fails
                let cursor = Cursor::new(&file);
                Sniffer::new()
                    .sniff_reader(cursor)
                    .map_or_else(|_| b',', |metadata| metadata.dialect.delimiter)
            }
        };

        let mut reader = csv::ReaderBuilder::new()
            .delimiter(delimiter)
            .has_headers(false)
            .flexible(true)
            .from_reader(file);

        let mut preview = vec![];
        for (i, entry) in reader.records().enumerate() {
            if i >= max_rows as usize {
                break;
            }
            match entry {
                Err(e) => return Err(error(format!("line {}: {}", i + 1, e))),
                Ok(record) => preview.push(record.iter().map(|s| s.to_string()).collect()),
            }
        }

        Ok(preview)
    }

    /// Imports a CSV file into the grid.
    pub fn import_csv_operations(
        &mut self,
        sheet_id: SheetId,
        file: Vec<u8>,
        file_name: &str,
        insert_at: Pos,
        delimiter: Option<u8>,
        header_is_first_row: Option<bool>,
    ) -> Result<Vec<Operation>> {
        let error = |message: String| anyhow!("Error parsing CSV file {}: {}", file_name, message);
        let sheet_pos = SheetPos::from((insert_at, sheet_id));

        let file: &[u8] = match String::from_utf8_lossy(&file) {
            Cow::Borrowed(_) => &file,
            Cow::Owned(_) => {
                if let Some(utf) = read_utf16(&file) {
                    return self.import_csv_operations(
                        sheet_id,
                        utf.as_bytes().to_vec(),
                        file_name,
                        insert_at,
                        delimiter,
                        header_is_first_row,
                    );
                }
                &file
            }
        };

        let delimiter = match delimiter {
            Some(d) => d,
            None => {
                // auto detect the delimiter, default to ',' if it fails
                let cursor = Cursor::new(&file);
                Sniffer::new()
                    .sniff_reader(cursor)
                    .map_or_else(|_| b',', |metadata| metadata.dialect.delimiter)
            }
        };

        let reader = |flexible| {
            csv::ReaderBuilder::new()
                .delimiter(delimiter)
                .has_headers(false)
                .flexible(flexible)
                .from_reader(file)
        };

        let height = reader(false).records().count() as u32;

        // since the first row or more can be headers, look at the width of the last row
        let width = reader(true)
            .records()
            .last()
            .iter()
            .flatten()
            .next()
            .map(|s| s.len())
            .unwrap_or(0) as u32;

        if width == 0 {
            bail!("empty files cannot be processed");
        }

        let array_size = ArraySize::new_or_err(width, height).map_err(|e| error(e.to_string()))?;
        let mut cell_values = Array::new_empty(array_size);
        let mut sheet_format_updates = SheetFormatUpdates::default();
        let mut y: u32 = 0;

        for entry in reader(true).records() {
            match entry {
                Err(e) => return Err(error(format!("line {}: {}", y + 1, e))),
                Ok(record) => {
                    for (x, value) in record.iter().enumerate() {
                        let (cell_value, format_update) = self.string_to_cell_value(value, false);

                        cell_values
                            .set(u32::try_from(x)?, y, cell_value)
                            .map_err(|e| error(e.to_string()))?;

                        if !format_update.is_default() {
                            let pos = Pos {
                                x: x as i64 + 1,
                                y: y as i64 + 1,
                            };
                            sheet_format_updates.set_format_cell(pos, format_update);
                        }
                    }
                }
            }

            y += 1;

            // update the progress bar every time there's a new batch
            let should_update = y % IMPORT_LINES_PER_OPERATION == 0;

            if should_update && (cfg!(target_family = "wasm") || cfg!(test)) {
                crate::wasm_bindings::js::jsImportProgress(
                    file_name,
                    y,
                    height,
                    insert_at.x,
                    insert_at.y,
                    width,
                    height,
                );
            }
        }

        let context = self.a1_context();
        let import = Import::new(file_name.into());
        let mut data_table =
            DataTable::from((import.to_owned(), Array::new_empty(array_size), context));

        let apply_first_row_as_header = match header_is_first_row {
            Some(true) => true,
            Some(false) => false,
            None => self.guess_csv_first_row_is_header(&cell_values),
        };

        data_table.value = cell_values.into();
        data_table.formats.apply_updates(&sheet_format_updates);

        if apply_first_row_as_header {
            data_table.apply_first_row_as_header();
        }

        drop(sheet_format_updates);

        let ops = vec![Operation::AddDataTable {
            sheet_pos,
            data_table,
            cell_value: CellValue::Import(import),
            index: None,
        }];

        Ok(ops)
    }

    /// Imports an Excel file into the grid.
    pub fn import_excel_operations(
        &mut self,
        file: &[u8],
        file_name: &str,
    ) -> Result<Vec<Operation>> {
        let mut ops = vec![] as Vec<Operation>;
        let error = |e: XlsxError| anyhow!("Error parsing Excel file {file_name}: {e}");

        let cursor = Cursor::new(file);
        let mut workbook: Xlsx<_> = ExcelReader::new(cursor).map_err(error)?;
        let sheets = workbook.sheet_names().to_owned();

        let existing_sheet_names = self.sheet_names();
        for sheet_name in sheets.iter() {
            if existing_sheet_names.contains(&sheet_name.as_str()) {
                bail!("Sheet with name {} already exists", sheet_name);
            }
        }

        let xlsx_range_to_pos = |(row, col)| Pos {
            x: col as i64 + 1,
            y: row as i64 + 1,
        };

        // total rows for calculating import progress
        let total_rows = sheets
            .iter()
            .try_fold(0, |acc, sheet_name| {
                let range = workbook.worksheet_range(sheet_name)?;
                // counted twice because we have to read values and formulas
                Ok(acc + 2 * range.rows().count())
            })
            .map_err(error)?;
        let mut current_y_values = 0;
        let mut current_y_formula = 0;

        let mut order = key_between(None, None).unwrap_or("A0".to_string());
        for sheet_name in sheets {
            // add the sheet
            let mut sheet = Sheet::new(SheetId::new(), sheet_name.to_owned(), order.clone());
            order = key_between(Some(&order), None).unwrap_or("A0".to_string());

            // values
            let range = workbook.worksheet_range(&sheet_name).map_err(error)?;
            let insert_at = range.start().map_or_else(|| pos![A1], xlsx_range_to_pos);
            for (y, row) in range.rows().enumerate() {
                for (x, cell) in row.iter().enumerate() {
                    let cell_value = match cell {
                        ExcelData::Empty => continue,
                        ExcelData::String(value) => CellValue::Text(value.to_string()),
                        ExcelData::DateTimeIso(value) => CellValue::unpack_date_time(value)
                            .unwrap_or(CellValue::Text(value.to_string())),
                        ExcelData::DateTime(value) => {
                            if value.is_datetime() {
                                value.as_datetime().map_or_else(
                                    || CellValue::Blank,
                                    |v| {
                                        // there's probably a better way to figure out if it's a Date or a DateTime, but this works for now
                                        if let (Ok(zero_time), Ok(zero_date)) = (
                                            NaiveTime::parse_from_str("00:00:00", "%H:%M:%S"),
                                            NaiveDate::parse_from_str("1899-12-31", "%Y-%m-%d"),
                                        ) {
                                            if v.time() == zero_time {
                                                CellValue::Date(v.date())
                                            } else if v.date() == zero_date {
                                                CellValue::Time(v.time())
                                            } else {
                                                CellValue::DateTime(v)
                                            }
                                        } else {
                                            CellValue::DateTime(v)
                                        }
                                    },
                                )
                            } else {
                                CellValue::Text(value.to_string())
                            }
                        }
                        ExcelData::DurationIso(value) => CellValue::Text(value.to_string()),
                        ExcelData::Float(value) => {
                            CellValue::unpack_str_float(&value.to_string(), CellValue::Blank)
                        }
                        ExcelData::Int(value) => {
                            CellValue::unpack_str_float(&value.to_string(), CellValue::Blank)
                        }
                        ExcelData::Error(_) => continue,
                        ExcelData::Bool(value) => CellValue::Logical(*value),
                    };

                    sheet.set_cell_value(
                        Pos {
                            x: insert_at.x + x as i64,
                            y: insert_at.y + y as i64,
                        },
                        cell_value,
                    );
                }

                // send progress to the client, every IMPORT_LINES_PER_OPERATION
                if (cfg!(target_family = "wasm") || cfg!(test))
                    && current_y_values % IMPORT_LINES_PER_OPERATION == 0
                {
                    let width = row.len() as u32;
                    crate::wasm_bindings::js::jsImportProgress(
                        file_name,
                        current_y_values + current_y_formula,
                        total_rows as u32,
                        0,
                        1,
                        width,
                        total_rows as u32,
                    );
                }
                current_y_values += 1;
            }

            // formulas
            let formula = workbook.worksheet_formula(&sheet_name).map_err(error)?;
            let insert_at = formula.start().map_or_else(Pos::default, xlsx_range_to_pos);
            let mut formula_compute_ops = vec![];
            for (y, row) in formula.rows().enumerate() {
                for (x, cell) in row.iter().enumerate() {
                    if !cell.is_empty() {
                        let pos = Pos {
                            x: insert_at.x + x as i64,
                            y: insert_at.y + y as i64,
                        };
                        let cell_value = CellValue::Code(CodeCellValue {
                            language: CodeCellLanguage::Formula,
                            code: cell.to_string(),
                        });
                        sheet.set_cell_value(pos, cell_value);
                        // add code compute operation, to generate code runs
                        formula_compute_ops.push(Operation::ComputeCode {
                            sheet_pos: pos.to_sheet_pos(sheet.id),
                        });
                    }
                }

                // send progress to the client, every IMPORT_LINES_PER_OPERATION
                if (cfg!(target_family = "wasm") || cfg!(test))
                    && current_y_formula % IMPORT_LINES_PER_OPERATION == 0
                {
                    let width = row.len() as u32;
                    crate::wasm_bindings::js::jsImportProgress(
                        file_name,
                        current_y_values + current_y_formula,
                        total_rows as u32,
                        0,
                        1,
                        width,
                        total_rows as u32,
                    );
                }
                current_y_formula += 1;
            }

            // add new sheets
            ops.push(Operation::AddSheetSchema {
                schema: Box::new(export_sheet(sheet)),
            });
            ops.extend(formula_compute_ops);
        }

        Ok(ops)
    }

    /// Imports a Parquet file into the grid.
    pub fn import_parquet_operations(
        &mut self,
        sheet_id: SheetId,
        file: Vec<u8>,
        file_name: &str,
        insert_at: Pos,
    ) -> Result<Vec<Operation>> {
        let error =
            |message: String| anyhow!("Error parsing Parquet file {}: {}", file_name, message);

        // this is not expensive
        let bytes = Bytes::from(file);
        let builder = ParquetRecordBatchReaderBuilder::try_new(bytes)?;

        // headers
        let metadata = builder.metadata();
        let total_size = metadata.file_metadata().num_rows() as u32;
        let fields = metadata.file_metadata().schema().get_fields();
        let headers: Vec<CellValue> = fields.iter().map(|f| f.name().into()).collect();
        let mut width = headers.len() as u32;

        // add 1 to the height for the headers
        let array_size =
            ArraySize::new_or_err(width, total_size + 1).map_err(|e| error(e.to_string()))?;
        let mut cell_values = Array::new_empty(array_size);

        // add the headers to the first row
        for (x, header) in headers.into_iter().enumerate() {
            cell_values
                .set(x as u32, 0, header)
                .map_err(|e| error(e.to_string()))?;
        }

        let reader = builder.build()?;
        let mut height = 0;
        let mut current_size = 0;

        for (row_index, batch) in reader.enumerate() {
            let batch = batch?;
            let num_rows = batch.num_rows();
            let num_cols = batch.num_columns();

            current_size += num_rows;
            width = width.max(num_cols as u32);
            height = height.max(num_rows as u32);

            for col_index in 0..num_cols {
                let col = batch.column(col_index);
                let values = arrow_col_to_cell_value_vec(col)?;
                let x = col_index as u32;
                let y = (row_index * num_rows) as u32 + 1;

                for (index, value) in values.into_iter().enumerate() {
                    cell_values
                        .set(x, y + index as u32, value)
                        .map_err(|e| error(e.to_string()))?;
                }

                // update the progress bar every time there's a new operation
                if cfg!(target_family = "wasm") || cfg!(test) {
                    crate::wasm_bindings::js::jsImportProgress(
                        file_name,
                        current_size as u32,
                        total_size,
                        insert_at.x,
                        insert_at.y,
                        width,
                        height,
                    );
                }
            }
        }

        let context = self.a1_context();
        let import = Import::new(file_name.into());
        let mut data_table = DataTable::from((import.to_owned(), cell_values, context));
        data_table.apply_first_row_as_header();

        let ops = vec![Operation::AddDataTable {
            sheet_pos: SheetPos::from((insert_at, sheet_id)),
            data_table,
            cell_value: CellValue::Import(import),
            index: None,
        }];

        Ok(ops)
    }
}

fn read_utf16(bytes: &[u8]) -> Option<String> {
    if bytes.is_empty() && bytes.len() % 2 == 0 {
        return None;
    }

    // convert u8 to u16
    let mut utf16vec: Vec<u16> = Vec::with_capacity(bytes.len() / 2);
    for chunk in bytes.to_owned().chunks_exact(2) {
        let Ok(vec2) = <[u8; 2]>::try_from(chunk) else {
            return None;
        };
        utf16vec.push(u16::from_ne_bytes(vec2));
    }

    // convert to string
    let Ok(str) = String::from_utf16(utf16vec.as_slice()) else {
        return None;
    };

    // strip invalid characters
    let result: String = str.chars().filter(|&c| c.len_utf8() <= 2).collect();

    Some(result)
}

#[cfg(test)]
mod test {
    use super::{read_utf16, *};
    use crate::{
        CellValue, controller::user_actions::import::tests::simple_csv_at,
        test_util::assert_display_cell_value,
    };
    use chrono::{NaiveDate, NaiveDateTime, NaiveTime};

    const INVALID_ENCODING_FILE: &[u8] =
        include_bytes!("../../../../quadratic-rust-shared/data/csv/encoding_issue.csv");

    #[test]
    fn guesses_the_csv_header() {
        let (gc, sheet_id, pos, _) = simple_csv_at(Pos { x: 1, y: 1 });
        let sheet = gc.sheet(sheet_id);
        let values = sheet.data_table(pos).unwrap().value_as_array().unwrap();
        assert!(gc.guess_csv_first_row_is_header(values));
    }

    #[test]
    fn test_get_csv_preview_with_valid_csv() {
        let csv_data = b"header1,header2\nvalue1,value2\nvalue3,value4";
        let result = GridController::get_csv_preview(csv_data.to_vec(), 6, Some(b','));
        assert!(result.is_ok());
        let preview = result.unwrap();
        assert_eq!(preview.len(), 3);
        assert_eq!(preview[0], vec!["header1", "header2"]);
        assert_eq!(preview[1], vec!["value1", "value2"]);
        assert_eq!(preview[2], vec!["value3", "value4"]);
    }

    #[test]
    fn test_get_csv_preview_with_auto_delimiter() {
        let csv_data = b"header1\theader2\nvalue1\tvalue2\nvalue3\tvalue4";
        let result = GridController::get_csv_preview(csv_data.to_vec(), 6, None);
        assert!(result.is_ok());
        let preview = result.unwrap();
        assert_eq!(preview.len(), 3);
        assert_eq!(preview[0], vec!["header1", "header2"]);
        assert_eq!(preview[1], vec!["value1", "value2"]);
        assert_eq!(preview[2], vec!["value3", "value4"]);
    }

    #[test]
    fn test_get_csv_preview_with_utf16() {
        let utf16_data: Vec<u8> = vec![
            0xFF, 0xFE, 0x68, 0x00, 0x65, 0x00, 0x61, 0x00, 0x64, 0x00, 0x65, 0x00, 0x72, 0x00,
            0x31, 0x00, 0x2C, 0x00, 0x68, 0x00, 0x65, 0x00, 0x61, 0x00, 0x64, 0x00, 0x65, 0x00,
            0x72, 0x00, 0x32, 0x00, 0x0A, 0x00, 0x76, 0x00, 0x61, 0x00, 0x6C, 0x00, 0x75, 0x00,
            0x65, 0x00, 0x31, 0x00, 0x2C, 0x00, 0x76, 0x00, 0x61, 0x00, 0x6C, 0x00, 0x75, 0x00,
            0x65, 0x00, 0x32, 0x00,
        ];
        let result = GridController::get_csv_preview(utf16_data, 6, Some(b','));
        assert!(result.is_ok());
        let preview = result.unwrap();
        assert_eq!(preview.len(), 2);
        assert_eq!(preview[0], vec!["header1", "header2"]);
        assert_eq!(preview[1], vec!["value1", "value2"]);
    }

    #[test]
    fn transmute_u8_to_u16() {
        let result = read_utf16(INVALID_ENCODING_FILE).unwrap();
        assert_eq!("issue, test, value\r\n0, 1, Invalid\r\n0, 2, Valid", result);
    }

    #[test]
    fn imports_a_simple_csv() {
        let mut gc = GridController::test();
        let sheet_id = gc.grid.sheets()[0].id;
        let pos = pos![A1];
        let file_name = "simple.csv";

        const SIMPLE_CSV: &str =
            "city,region,country,population\nSouthborough,MA,United States,a lot of people";

        let ops = gc
            .import_csv_operations(
                sheet_id,
                SIMPLE_CSV.as_bytes().to_vec(),
                file_name,
                pos,
                Some(b','),
                Some(false),
            )
            .unwrap();

        let values = vec![
            vec!["city", "region", "country", "population"],
            vec!["Southborough", "MA", "United States", "a lot of people"],
        ];
        let context = gc.a1_context();
        let import = Import::new(file_name.into());
        let cell_value = CellValue::Import(import.clone());
        let mut expected_data_table = DataTable::from((import, values.into(), context));
        assert_display_cell_value(&gc, sheet_id, 1, 1, &cell_value.to_string());

        let data_table = match ops[0].clone() {
            Operation::AddDataTable { data_table, .. } => data_table,
            _ => panic!("Expected AddDataTable operation"),
        };
        expected_data_table.last_modified = data_table.last_modified;
        expected_data_table.name = CellValue::Text(file_name.to_string());

        let expected = Operation::AddDataTable {
            sheet_pos: SheetPos::new(sheet_id, 1, 1),
            data_table: expected_data_table,
            cell_value,
            index: None,
        };

        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0], expected);
    }

    #[test]
    fn imports_a_long_csv() {
        let mut gc = GridController::test();
        let sheet_id = gc.grid.sheets()[0].id;
        let pos = Pos { x: 1, y: 2 };
        let file_name = "long.csv";

        let mut csv = String::new();
        for i in 0..IMPORT_LINES_PER_OPERATION * 2 + 150 {
            csv.push_str(&format!("city{},MA,United States,{}\n", i, i * 1000));
        }

        let ops = gc.import_csv_operations(
            sheet_id,
            csv.as_bytes().to_vec(),
            file_name,
            pos,
            Some(b','),
            Some(false),
        );

        let import = Import::new(file_name.into());
        let cell_value = CellValue::Import(import.clone());
        assert_display_cell_value(&gc, sheet_id, 0, 0, &cell_value.to_string());

        assert_eq!(ops.as_ref().unwrap().len(), 1);

        let (sheet_pos, data_table) = match &ops.unwrap()[0] {
            Operation::AddDataTable {
                sheet_pos,
                data_table,
                ..
            } => (*sheet_pos, data_table.clone()),
            _ => panic!("Expected AddDataTable operation"),
        };
        assert_eq!(sheet_pos.x, 1);
        assert_eq!(
            data_table.cell_value_ref_at(0, 2),
            Some(&CellValue::Text("city0".into()))
        );
    }

    #[test]
    fn import_csv_date_time() {
        let mut gc = GridController::test();
        let sheet_id = gc.grid.sheets()[0].id;

        let pos = pos![A1];
        let csv = "2024-12-21,13:23:00,2024-12-21 13:23:00\n".to_string();
        gc.import_csv(
            sheet_id,
            csv.as_bytes().to_vec(),
            "csv",
            pos,
            None,
            Some(b','),
            Some(false),
        )
        .unwrap();

        let value = CellValue::Date(NaiveDate::parse_from_str("2024-12-21", "%Y-%m-%d").unwrap());
        assert_display_cell_value(&gc, sheet_id, 1, 3, &value.to_string());

        let value = CellValue::Time(NaiveTime::parse_from_str("13:23:00", "%H:%M:%S").unwrap());
        assert_display_cell_value(&gc, sheet_id, 2, 3, &value.to_string());

        let value = CellValue::DateTime(
            NaiveDate::from_ymd_opt(2024, 12, 21)
                .unwrap()
                .and_hms_opt(13, 23, 0)
                .unwrap(),
        );
        assert_display_cell_value(&gc, sheet_id, 3, 3, &value.to_string());
    }

    #[test]
    fn import_excel() {
        let mut gc = GridController::new_blank();
        let file = include_bytes!("../../../test-files/simple.xlsx");
        gc.import_excel(file.as_ref(), "simple.xlsx", None).unwrap();

        let sheet_id = gc.grid.sheets()[0].id;
        let sheet = gc.sheet(sheet_id);

        assert_eq!(
            sheet.cell_value((1, 1).into()),
            Some(CellValue::Number(1.into()))
        );
        assert_eq!(
            sheet.cell_value((3, 10).into()),
            Some(CellValue::Number(12.into()))
        );
        assert_eq!(sheet.cell_value((1, 6).into()), None);
        assert_eq!(
            sheet.cell_value((4, 2).into()),
            Some(CellValue::Code(CodeCellValue {
                language: CodeCellLanguage::Formula,
                code: "C1:C5".into()
            }))
        );
        assert_eq!(sheet.cell_value((4, 1).into()), None);
    }

    #[test]
    fn import_excel_invalid() {
        let mut gc = GridController::new_blank();
        let file = include_bytes!("../../../test-files/invalid.xlsx");
        let result = gc.import_excel(file.as_ref(), "invalid.xlsx", None);
        assert!(result.is_err());
    }

    #[test]
    fn import_parquet_date_time() {
        let mut gc = GridController::test();
        let sheet_id = gc.grid.sheets()[0].id;
        let file = include_bytes!("../../../test-files/date_time_formats_arrow.parquet");
        let pos = pos![A1];
        gc.import_parquet(sheet_id, file.to_vec(), "parquet", pos, None)
            .unwrap();

        let sheet = gc.sheet(sheet_id);
        let data_table = sheet.data_table(pos).unwrap();

        // date
        assert_eq!(
            data_table.cell_value_at(0, 2),
            Some(CellValue::Date(
                NaiveDate::parse_from_str("2024-12-21", "%Y-%m-%d").unwrap()
            ))
        );
        assert_eq!(
            data_table.cell_value_at(0, 3),
            Some(CellValue::Date(
                NaiveDate::parse_from_str("2024-12-22", "%Y-%m-%d").unwrap()
            ))
        );
        assert_eq!(
            data_table.cell_value_at(0, 4),
            Some(CellValue::Date(
                NaiveDate::parse_from_str("2024-12-23", "%Y-%m-%d").unwrap()
            ))
        );

        // time
        assert_eq!(
            data_table.cell_value_at(1, 2),
            Some(CellValue::Time(
                NaiveTime::parse_from_str("13:23:00", "%H:%M:%S").unwrap()
            ))
        );
        assert_eq!(
            data_table.cell_value_at(1, 3),
            Some(CellValue::Time(
                NaiveTime::parse_from_str("14:45:00", "%H:%M:%S").unwrap()
            ))
        );
        assert_eq!(
            data_table.cell_value_at(1, 4),
            Some(CellValue::Time(
                NaiveTime::parse_from_str("16:30:00", "%H:%M:%S").unwrap()
            ))
        );

        // date time
        assert_eq!(
            data_table.cell_value_at(2, 2),
            Some(CellValue::DateTime(
                NaiveDate::from_ymd_opt(2024, 12, 21)
                    .unwrap()
                    .and_hms_opt(13, 23, 0)
                    .unwrap()
            ))
        );
        assert_eq!(
            data_table.cell_value_at(2, 3),
            Some(CellValue::DateTime(
                NaiveDate::from_ymd_opt(2024, 12, 22)
                    .unwrap()
                    .and_hms_opt(14, 30, 0)
                    .unwrap()
            ))
        );
        assert_eq!(
            data_table.cell_value_at(2, 4),
            Some(CellValue::DateTime(
                NaiveDate::from_ymd_opt(2024, 12, 23)
                    .unwrap()
                    .and_hms_opt(16, 45, 0)
                    .unwrap()
            ))
        );
    }

    #[test]
    fn import_excel_date_time() {
        let mut gc = GridController::new_blank();
        let file = include_bytes!("../../../test-files/date_time.xlsx");
        gc.import_excel(file.as_ref(), "excel", None).unwrap();

        let sheet_id = gc.grid.sheets()[0].id;
        let sheet = gc.sheet(sheet_id);

        // date
        assert_eq!(
            sheet.cell_value((1, 2).into()),
            Some(CellValue::Date(
                NaiveDate::parse_from_str("1990-12-21", "%Y-%m-%d").unwrap()
            ))
        );
        assert_eq!(
            sheet.cell_value((1, 3).into()),
            Some(CellValue::Date(
                NaiveDate::parse_from_str("1990-12-22", "%Y-%m-%d").unwrap()
            ))
        );
        assert_eq!(
            sheet.cell_value((1, 4).into()),
            Some(CellValue::Date(
                NaiveDate::parse_from_str("1990-12-23", "%Y-%m-%d").unwrap()
            ))
        );

        // date time
        assert_eq!(
            sheet.cell_value((2, 2).into()),
            Some(CellValue::DateTime(
                NaiveDateTime::parse_from_str("2021-1-5 15:45", "%Y-%m-%d %H:%M").unwrap()
            ))
        );
        assert_eq!(
            sheet.cell_value((2, 3).into()),
            Some(CellValue::DateTime(
                NaiveDateTime::parse_from_str("2021-1-6 15:45", "%Y-%m-%d %H:%M").unwrap()
            ))
        );
        assert_eq!(
            sheet.cell_value((2, 4).into()),
            Some(CellValue::DateTime(
                NaiveDateTime::parse_from_str("2021-1-7 15:45", "%Y-%m-%d %H:%M").unwrap()
            ))
        );

        // time
        assert_eq!(
            sheet.cell_value((3, 2).into()),
            Some(CellValue::Time(
                NaiveTime::parse_from_str("13:23:00", "%H:%M:%S").unwrap()
            ))
        );
        assert_eq!(
            sheet.cell_value((3, 3).into()),
            Some(CellValue::Time(
                NaiveTime::parse_from_str("14:23:00", "%H:%M:%S").unwrap()
            ))
        );
        assert_eq!(
            sheet.cell_value((3, 4).into()),
            Some(CellValue::Time(
                NaiveTime::parse_from_str("15:23:00", "%H:%M:%S").unwrap()
            ))
        );
    }
}
