//! A report table as the site reads it: columns and rows of JSON values,
//! whether it came from an Arrow batch over the data socket or from a
//! fixture file. Cells are looked up by column name; a missing column is
//! `None`, never a panic, because a plan's report may omit a table a
//! newer backtester writes.

use arrow::array::{Array, AsArray, RecordBatch};
use arrow::datatypes::{DataType, Float64Type, Int32Type, Int64Type, UInt32Type, UInt64Type};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Table {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
}

impl Table {
    pub fn new(columns: &[&str]) -> Table {
        Table {
            columns: columns.iter().map(|c| c.to_string()).collect(),
            rows: Vec::new(),
        }
    }

    pub fn push(&mut self, row: Vec<Value>) {
        debug_assert_eq!(row.len(), self.columns.len());
        self.rows.push(row);
    }

    pub fn col(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|c| c == name)
    }

    pub fn cell(&self, row: usize, name: &str) -> Option<&Value> {
        let c = self.col(name)?;
        self.rows.get(row)?.get(c)
    }

    pub fn f64(&self, row: usize, name: &str) -> Option<f64> {
        match self.cell(row, name)? {
            Value::Number(n) => n.as_f64(),
            Value::String(s) => s.parse().ok(),
            _ => None,
        }
    }

    pub fn str(&self, row: usize, name: &str) -> Option<&str> {
        self.cell(row, name)?.as_str()
    }

    pub fn text(&self, row: usize, name: &str) -> String {
        match self.cell(row, name) {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Null) | None => String::new(),
            Some(v) => v.to_string(),
        }
    }

    /// The indices of the rows whose `name` column is the string `value`.
    pub fn rows_where(&self, name: &str, value: &str) -> Vec<usize> {
        (0..self.rows.len()).filter(|&i| self.str(i, name) == Some(value)).collect()
    }

    /// The first row whose `name` column is `value`.
    pub fn find(&self, name: &str, value: &str) -> Option<usize> {
        self.rows_where(name, value).into_iter().next()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// The table of the batches a `scan` answered, every cell as JSON: numbers
    /// as numbers, times as ISO-8601, labels and dictionaries as strings,
    /// anything else through Arrow's own display.
    pub fn from_batches(batches: &[RecordBatch]) -> Table {
        let Some(first) = batches.first() else { return Table::default() };
        let columns: Vec<String> = first.schema().fields().iter().map(|f| f.name().clone()).collect();
        let mut rows = Vec::new();
        for b in batches {
            let cols: Vec<Vec<Value>> = b.columns().iter().map(|a| column_values(a.as_ref())).collect();
            for r in 0..b.num_rows() {
                rows.push(cols.iter().map(|c| c[r].clone()).collect());
            }
        }
        Table { columns, rows }
    }
}

fn column_values(a: &dyn Array) -> Vec<Value> {
    let n = a.len();
    let null = |i: usize| a.is_null(i);
    match a.data_type() {
        DataType::Utf8 => {
            let s = a.as_string::<i32>();
            (0..n).map(|i| if null(i) { Value::Null } else { Value::String(s.value(i).to_string()) }).collect()
        }
        DataType::LargeUtf8 => {
            let s = a.as_string::<i64>();
            (0..n).map(|i| if null(i) { Value::Null } else { Value::String(s.value(i).to_string()) }).collect()
        }
        DataType::Float64 => {
            let p = a.as_primitive::<Float64Type>();
            (0..n)
                .map(|i| {
                    if null(i) {
                        Value::Null
                    } else {
                        serde_json::Number::from_f64(p.value(i)).map(Value::Number).unwrap_or(Value::Null)
                    }
                })
                .collect()
        }
        DataType::Int64 => {
            let p = a.as_primitive::<Int64Type>();
            (0..n).map(|i| if null(i) { Value::Null } else { Value::from(p.value(i)) }).collect()
        }
        DataType::Int32 => {
            let p = a.as_primitive::<Int32Type>();
            (0..n).map(|i| if null(i) { Value::Null } else { Value::from(p.value(i)) }).collect()
        }
        DataType::UInt64 => {
            let p = a.as_primitive::<UInt64Type>();
            (0..n).map(|i| if null(i) { Value::Null } else { Value::from(p.value(i)) }).collect()
        }
        DataType::UInt32 => {
            let p = a.as_primitive::<UInt32Type>();
            (0..n).map(|i| if null(i) { Value::Null } else { Value::from(p.value(i)) }).collect()
        }
        DataType::Boolean => {
            let p = a.as_boolean();
            (0..n).map(|i| if null(i) { Value::Null } else { Value::Bool(p.value(i)) }).collect()
        }
        DataType::Timestamp(unit, _) => {
            let scale = match unit {
                arrow::datatypes::TimeUnit::Second => 1_000_000_000,
                arrow::datatypes::TimeUnit::Millisecond => 1_000_000,
                arrow::datatypes::TimeUnit::Microsecond => 1_000,
                arrow::datatypes::TimeUnit::Nanosecond => 1,
            };
            let p = arrow::compute::cast(a, &DataType::Int64).ok();
            (0..n)
                .map(|i| match &p {
                    Some(p) if !null(i) => Value::String(pipeline_wire::iso(p.as_primitive::<Int64Type>().value(i) * scale)),
                    _ => Value::Null,
                })
                .collect()
        }
        DataType::Dictionary(_, _) => match arrow::compute::cast(a, &DataType::Utf8) {
            Ok(c) => column_values(c.as_ref()),
            Err(_) => displayed(a),
        },
        _ => displayed(a),
    }
}

fn displayed(a: &dyn Array) -> Vec<Value> {
    let f = arrow::util::display::ArrayFormatter::try_new(a, &arrow::util::display::FormatOptions::default());
    (0..a.len())
        .map(|i| match &f {
            Ok(f) if !a.is_null(i) => Value::String(f.value(i).to_string()),
            _ => Value::Null,
        })
        .collect()
}
