//! Tables for the model (contracts section 5, PL-D5): an Arrow table as
//! compact JSON, `{"columns": [...], "rows": [[...]], "truncated": bool}`,
//! at most `ROW_CAP` rows; and a Markdown rendering for people in the logs
//! (`--markdown`). Times are ISO-8601, labels their text, hashes their hex.

use arrow::array::{Array, AsArray, RecordBatch};
use arrow::datatypes::{DataType, TimeUnit};
use arrow::util::display::{ArrayFormatter, FormatOptions};
use serde::Serialize;
use serde_json::Value;

/// The most rows a table carries to the model (PL-D5).
pub const ROW_CAP: usize = 500;

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct Table {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
    pub truncated: bool,
}

impl Table {
    /// The first `cap` rows of the batches.
    pub fn of(batches: &[RecordBatch], cap: usize) -> Table {
        let mut t = Table::default();
        if let Some(b) = batches.first() {
            t.columns = b.schema().fields().iter().map(|f| f.name().clone()).collect();
        }
        let total: usize = batches.iter().map(|b| b.num_rows()).sum();
        let mut left = cap;
        for b in batches {
            if left == 0 {
                break;
            }
            let n = b.num_rows().min(left);
            let cols: Vec<Vec<Value>> = b.columns().iter().map(|c| cells(c.as_ref(), n)).collect();
            for i in 0..n {
                t.rows.push(cols.iter().map(|c| c[i].clone()).collect());
            }
            left -= n;
        }
        t.truncated = total > cap;
        t
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// The value of the first row's column `name`.
    pub fn first(&self, name: &str) -> Option<&Value> {
        let i = self.columns.iter().position(|c| c == name)?;
        self.rows.first().and_then(|r| r.get(i))
    }

    /// The column `name` of every row.
    pub fn column(&self, name: &str) -> Vec<&Value> {
        match self.columns.iter().position(|c| c == name) {
            Some(i) => self.rows.iter().filter_map(|r| r.get(i)).collect(),
            None => vec![],
        }
    }

    /// The rows as objects keyed by column, for the routes that answer a
    /// list of records.
    pub fn records(&self) -> Vec<serde_json::Map<String, Value>> {
        self.rows.iter().map(|r| self.columns.iter().cloned().zip(r.iter().cloned()).collect()).collect()
    }

    /// A Markdown table, for people.
    pub fn markdown(&self) -> String {
        let cell = |v: &Value| match v {
            Value::String(s) => s.replace('|', "\\|").replace('\n', " "),
            Value::Null => String::new(),
            other => other.to_string(),
        };
        let mut s = String::new();
        s.push_str(&format!("| {} |\n", self.columns.join(" | ")));
        s.push_str(&format!("|{}\n", self.columns.iter().map(|_| " --- |").collect::<String>()));
        for r in &self.rows {
            s.push_str(&format!("| {} |\n", r.iter().map(cell).collect::<Vec<_>>().join(" | ")));
        }
        if self.truncated {
            s.push_str(&format!("\n(the first {} rows; narrow the query for the rest)\n", self.rows.len()));
        }
        s
    }
}

/// The bytes a table's batches hold, what the bytes budget meters.
pub fn bytes_of(batches: &[RecordBatch]) -> u64 {
    batches.iter().map(|b| b.get_array_memory_size() as u64).sum()
}

fn cells(a: &dyn Array, n: usize) -> Vec<Value> {
    let mut out = Vec::with_capacity(n);
    match a.data_type() {
        DataType::Utf8 => {
            let s = a.as_string::<i32>();
            for i in 0..n {
                out.push(if s.is_null(i) { Value::Null } else { Value::String(s.value(i).to_string()) });
            }
        }
        DataType::LargeUtf8 => {
            let s = a.as_string::<i64>();
            for i in 0..n {
                out.push(if s.is_null(i) { Value::Null } else { Value::String(s.value(i).to_string()) });
            }
        }
        DataType::Binary => {
            let s = a.as_binary::<i32>();
            for i in 0..n {
                out.push(if s.is_null(i) {
                    Value::Null
                } else {
                    Value::String(String::from_utf8_lossy(s.value(i)).into_owned())
                });
            }
        }
        DataType::Boolean => {
            let b = a.as_boolean();
            for i in 0..n {
                out.push(if b.is_null(i) { Value::Null } else { Value::Bool(b.value(i)) });
            }
        }
        DataType::Int64 => {
            let x = a.as_primitive::<arrow::datatypes::Int64Type>();
            for i in 0..n {
                out.push(if x.is_null(i) { Value::Null } else { Value::from(x.value(i)) });
            }
        }
        DataType::Int32 => {
            let x = a.as_primitive::<arrow::datatypes::Int32Type>();
            for i in 0..n {
                out.push(if x.is_null(i) { Value::Null } else { Value::from(x.value(i)) });
            }
        }
        DataType::UInt64 => {
            let x = a.as_primitive::<arrow::datatypes::UInt64Type>();
            for i in 0..n {
                out.push(if x.is_null(i) { Value::Null } else { Value::from(x.value(i)) });
            }
        }
        DataType::UInt32 => {
            let x = a.as_primitive::<arrow::datatypes::UInt32Type>();
            for i in 0..n {
                out.push(if x.is_null(i) { Value::Null } else { Value::from(x.value(i)) });
            }
        }
        DataType::Float64 => {
            let x = a.as_primitive::<arrow::datatypes::Float64Type>();
            for i in 0..n {
                out.push(if x.is_null(i) { Value::Null } else { float(x.value(i)) });
            }
        }
        DataType::Float32 => {
            let x = a.as_primitive::<arrow::datatypes::Float32Type>();
            for i in 0..n {
                out.push(if x.is_null(i) { Value::Null } else { float(x.value(i) as f64) });
            }
        }
        DataType::Timestamp(TimeUnit::Nanosecond, _) => {
            let x = a.as_primitive::<arrow::datatypes::TimestampNanosecondType>();
            for i in 0..n {
                out.push(if x.is_null(i) { Value::Null } else { Value::String(pipeline_wire::iso(x.value(i))) });
            }
        }
        DataType::Dictionary(_, _) => {
            // Labels: the text of each key.
            let opts = FormatOptions::default().with_null("");
            match ArrayFormatter::try_new(a, &opts) {
                Ok(f) => {
                    for i in 0..n {
                        out.push(if a.is_null(i) { Value::Null } else { Value::String(f.value(i).to_string()) });
                    }
                }
                Err(_) => out.resize(n, Value::Null),
            }
        }
        _ => {
            let opts = FormatOptions::default().with_null("");
            match ArrayFormatter::try_new(a, &opts) {
                Ok(f) => {
                    for i in 0..n {
                        out.push(if a.is_null(i) { Value::Null } else { Value::String(f.value(i).to_string()) });
                    }
                }
                Err(_) => out.resize(n, Value::Null),
            }
        }
    }
    out
}

fn float(x: f64) -> Value {
    if x.is_finite() {
        serde_json::Number::from_f64(x).map(Value::Number).unwrap_or(Value::Null)
    } else {
        Value::String(x.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arrow::array::{Float64Array, Int64Array, StringArray, TimestampNanosecondArray};
    use arrow::datatypes::{Field, Schema};
    use std::sync::Arc;

    fn batch(n: usize) -> RecordBatch {
        let schema = Arc::new(Schema::new(vec![
            Field::new("a", DataType::Int64, false),
            Field::new("t", DataType::Timestamp(TimeUnit::Nanosecond, None), false),
            Field::new("p", DataType::Float64, true),
            Field::new("s", DataType::Utf8, true),
        ]));
        RecordBatch::try_new(
            schema,
            vec![
                Arc::new(Int64Array::from((0..n as i64).collect::<Vec<_>>())),
                Arc::new(TimestampNanosecondArray::from(vec![1_700_000_000_000_000_000i64; n])),
                Arc::new(Float64Array::from((0..n).map(|i| if i == 0 { None } else { Some(i as f64 / 2.0) }).collect::<Vec<_>>())),
                Arc::new(StringArray::from((0..n).map(|i| Some(format!("s{}", i))).collect::<Vec<_>>())),
            ],
        )
        .unwrap()
    }

    #[test]
    fn a_table_renders_its_cells_and_caps_its_rows() {
        let t = Table::of(&[batch(3)], ROW_CAP);
        assert_eq!(t.columns, vec!["a", "t", "p", "s"]);
        assert_eq!(
            t.rows[1],
            vec![Value::from(1), Value::String("2023-11-14T22:13:20Z".into()), Value::from(0.5), Value::String("s1".into())]
        );
        assert_eq!(t.rows[0][2], Value::Null);
        assert!(!t.truncated);
        let t = Table::of(&[batch(600), batch(10)], ROW_CAP);
        assert_eq!(t.len(), ROW_CAP);
        assert!(t.truncated);
        let md = Table::of(&[batch(2)], ROW_CAP).markdown();
        assert!(md.starts_with("| a | t | p | s |\n| --- | --- | --- | --- |\n| 0 | 2023-11-14T22:13:20Z |  | s0 |\n"), "{}", md);
        assert!(Table::of(&[], ROW_CAP).is_empty());
    }
}
