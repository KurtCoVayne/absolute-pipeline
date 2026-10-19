//! The query forms of P-3.2 as the tool API takes them (contracts section
//! 5): a JSON form rendered as the owned `QueryForm`, a predicate in the
//! text of D-4.5 (`root = ES and expiry >= 2026-01-01`), a time as ISO-8601
//! or nanoseconds, an identity as an entity id or a label; and the clamp
//! of PL-7.1 on every request: `r_max := min(request, H)`, `as_of_seq :=
//! seq(H)`.

use abt_wire::generated::abt::wire::{
    Atom, Catalog, Chain, ChainFilters, Constant, ConstantKind, CountRows, Disjunction, Distinct, Duration, Exists, Identity, Latest, Op, Point, Predicate, QueryForm, QueryRequest, ReceiptFilter,
    Scan, Section, Series, View,
};
use serde_json::Value;

/// A request the gateway could not render: the agent's error (400).
#[derive(Debug, Clone, thiserror::Error)]
#[error("{0}")]
pub struct Malformed(pub String);

/// A time from JSON: ISO-8601 (`2026-09-18`, `2026-09-18T19:12:36Z`) or
/// nanoseconds since the epoch.
pub fn time_of(v: &Value, what: &str) -> Result<i64, Malformed> {
    match v {
        Value::String(s) => abt_wire::scalar::parse_timestamp(s)
            .or_else(|| pipeline_wire::parse_iso(s).ok())
            .ok_or_else(|| Malformed(format!("{}: `{}` is not a time", what, s))),
        Value::Number(n) => n.as_i64().ok_or_else(|| Malformed(format!("{}: `{}` is not a time", what, n))),
        other => Err(Malformed(format!("{}: `{}` is not a time", what, other))),
    }
}

fn opt_time(body: &Value, key: &str) -> Result<Option<i64>, Malformed> {
    match body.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => time_of(v, key).map(Some),
    }
}

fn need_time(body: &Value, key: &str) -> Result<i64, Malformed> {
    opt_time(body, key)?.ok_or_else(|| Malformed(format!("`{}` is required", key)))
}

fn need_str<'a>(body: &'a Value, key: &str) -> Result<&'a str, Malformed> {
    body.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| Malformed(format!("`{}` is required", key)))
}

/// `[t0, t1]` of a form: both given, or the whole axis.
fn window(body: &Value) -> Result<(i64, i64), Malformed> {
    Ok((opt_time(body, "t0")?.unwrap_or(i64::MIN), opt_time(body, "t1")?.unwrap_or(i64::MAX)))
}

/// An identity: a number is an entity id, a string a label (E-16).
pub fn identity(v: &Value) -> Result<Identity, Malformed> {
    let one = |x: &Value| -> Result<Constant, Malformed> {
        match x {
            Value::Number(n) => Ok(Constant {
                kind: ConstantKind::Entity,
                num: 0.0,
                int: n.as_i64().ok_or_else(|| Malformed(format!("`{}` is not an entity id", n)))?,
                text: None,
            }),
            Value::String(s) => Ok(Constant {
                kind: ConstantKind::Label,
                num: 0.0,
                int: 0,
                text: Some(s.clone()),
            }),
            other => Err(Malformed(format!("`{}` is not an identity: an entity id or a label", other))),
        }
    };
    let values = match v {
        Value::Array(a) => a.iter().map(one).collect::<Result<Vec<_>, _>>()?,
        other => vec![one(other)?],
    };
    if values.is_empty() {
        return Err(Malformed("`identity` is empty".into()));
    }
    Ok(Identity { values })
}

/// A constant of a predicate atom from its text: a time, a number, a
/// count, a bool, else a label (quotes dropped).
fn constant(s: &str) -> Constant {
    let s = s.trim().trim_matches('"').trim_matches('\'');
    if let Ok(b) = s.parse::<bool>() {
        return Constant {
            kind: ConstantKind::Bool,
            num: 0.0,
            int: b as i64,
            text: None,
        };
    }
    if s.len() >= 10 && s.as_bytes()[4] == b'-' {
        if let Some(t) = abt_wire::scalar::parse_timestamp(s) {
            return Constant {
                kind: ConstantKind::Time,
                num: 0.0,
                int: t,
                text: None,
            };
        }
    }
    if let Ok(i) = s.replace('_', "").parse::<i64>() {
        return Constant {
            kind: ConstantKind::Count,
            num: i as f64,
            int: i,
            text: None,
        };
    }
    if let Ok(x) = s.replace('_', "").parse::<f64>() {
        return Constant {
            kind: ConstantKind::Num,
            num: x,
            int: x as i64,
            text: None,
        };
    }
    Constant {
        kind: ConstantKind::Label,
        num: 0.0,
        int: 0,
        text: Some(s.to_string()),
    }
}

/// One atom: `column op value`, `column in (a, b)`, `column is null`.
fn atom(s: &str) -> Result<Atom, Malformed> {
    let s = s.trim();
    let bad = || Malformed(format!("`{}` is not an atom: column op value, column in (values) or column is null", s));
    if let Some((col, _)) = s.split_once(" is null") {
        return Ok(Atom {
            column: col.trim().to_string(),
            op: Op::IsNull,
            values: None,
        });
    }
    if let Some((col, rest)) = s.split_once(" in ") {
        let inner = rest.trim().strip_prefix('(').and_then(|r| r.strip_suffix(')')).ok_or_else(bad)?;
        let values: Vec<Constant> = inner.split(',').map(constant).collect();
        return Ok(Atom {
            column: col.trim().to_string(),
            op: Op::In,
            values: Some(values),
        });
    }
    for (text, op) in [(">=", Op::Ge), ("<=", Op::Le), ("!=", Op::Ne), ("<>", Op::Ne), ("=", Op::Eq), (">", Op::Gt), ("<", Op::Lt)] {
        if let Some(i) = s.find(text) {
            let (col, val) = (&s[..i], &s[i + text.len()..]);
            if col.trim().is_empty() || val.trim().is_empty() {
                return Err(bad());
            }
            return Ok(Atom {
                column: col.trim().to_string(),
                op,
                values: Some(vec![constant(val)]),
            });
        }
    }
    Err(bad())
}

/// A predicate (Definition D-4.1) from its text: atoms joined by `and`,
/// each an alternative joined by `or`.
pub fn predicate(text: &str) -> Result<Option<Predicate>, Malformed> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    let mut all = Vec::new();
    for conj in text.split(" and ") {
        let atoms = conj.split(" or ").map(atom).collect::<Result<Vec<_>, _>>()?;
        all.push(Disjunction { atoms });
    }
    Ok(Some(Predicate { all: Some(all) }))
}

fn opt_predicate(body: &Value) -> Result<Option<Box<Predicate>>, Malformed> {
    match body.get("predicate") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(predicate(s)?.map(Box::new)),
        Some(other) => Err(Malformed(format!("`predicate` is a text, not `{}`", other))),
    }
}

fn columns(body: &Value) -> Result<Option<Vec<String>>, Malformed> {
    match body.get("columns") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(a)) => Ok(Some(
            a.iter()
                .map(|c| c.as_str().map(str::to_string).ok_or_else(|| Malformed(format!("`{}` is not a column name", c))))
                .collect::<Result<Vec<_>, _>>()?,
        )),
        Some(other) => Err(Malformed(format!("`columns` is a list of names, not `{}`", other))),
    }
}

/// A duration of a chain filter: `30d`, `2mo`, `1y`.
fn duration(v: &Value) -> Result<Duration, Malformed> {
    let s = v.as_str().ok_or_else(|| Malformed(format!("`{}` is not a duration (30d, 2mo, 1y)", v)))?.trim();
    let (n, unit) = s.split_at(s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len()));
    let n: i64 = n.parse().map_err(|_| Malformed(format!("`{}` is not a duration (30d, 2mo, 1y)", s)))?;
    Ok(match unit {
        "d" => Duration { months: 0, days: n, nanos: 0 },
        "mo" => Duration { months: n, days: 0, nanos: 0 },
        "y" => Duration { months: 12 * n, days: 0, nanos: 0 },
        "h" => Duration {
            months: 0,
            days: 0,
            nanos: n * 3_600_000_000_000,
        },
        _ => return Err(Malformed(format!("`{}` is not a duration (30d, 2mo, 1y)", s))),
    })
}

/// The owned form of a JSON `form` (contracts section 5's `query` body).
pub fn form(body: &Value) -> Result<QueryForm, Malformed> {
    let kind = need_str(body, "kind")?;
    let relation = || need_str(body, "relation").map(str::to_string);
    let ident = || body.get("identity").map(identity).transpose().map(|i| i.map(Box::new));
    Ok(match kind {
        "point" => QueryForm::Point(Box::new(Point {
            relation: relation()?,
            identity: ident()?,
            t: need_time(body, "t")?,
        })),
        "latest" => QueryForm::Latest(Box::new(Latest {
            relation: relation()?,
            identity: ident()?,
            t_max: opt_time(body, "t_max")?.or(opt_time(body, "t")?).unwrap_or(i64::MAX),
        })),
        "series" => {
            let (t0, t1) = window(body)?;
            QueryForm::Series(Box::new(Series {
                relation: relation()?,
                identity: ident()?,
                t0,
                t1,
            }))
        }
        "section" => QueryForm::Section(Box::new(Section {
            relation: relation()?,
            t: need_time(body, "t")?,
        })),
        "scan" => {
            let (t0, t1) = window(body)?;
            QueryForm::Scan(Box::new(Scan {
                relation: relation()?,
                t0,
                t1,
                predicate: opt_predicate(body)?,
                columns: columns(body)?,
            }))
        }
        "exists" => {
            let (t0, t1) = window(body)?;
            QueryForm::Exists(Box::new(Exists {
                relation: relation()?,
                t0,
                t1,
                predicate: opt_predicate(body)?,
            }))
        }
        "count" => {
            let (t0, t1) = window(body)?;
            QueryForm::CountRows(Box::new(CountRows {
                relation: relation()?,
                t0,
                t1,
                predicate: opt_predicate(body)?,
            }))
        }
        "distinct" => {
            let (t0, t1) = window(body)?;
            QueryForm::Distinct(Box::new(Distinct {
                relation: relation()?,
                column: need_str(body, "column")?.to_string(),
                t0,
                t1,
            }))
        }
        "chain" => {
            let underlying = body.get("underlying").and_then(Value::as_u64).ok_or_else(|| Malformed("`underlying` is an entity id".into()))?;
            let f = body.get("filters").cloned().unwrap_or(Value::Object(Default::default()));
            let num = |k: &str| f.get(k).and_then(Value::as_f64);
            let text = |k: &str| f.get(k).and_then(Value::as_str).map(str::to_string);
            QueryForm::Chain(Box::new(Chain {
                underlying,
                t: need_time(body, "t")?,
                filters: Some(Box::new(ChainFilters {
                    expiry_from: f.get("expiry_from").map(duration).transpose()?.map(Box::new),
                    expiry_to: f.get("expiry_to").map(duration).transpose()?.map(Box::new),
                    moneyness_lo: num("moneyness_lo"),
                    moneyness_hi: num("moneyness_hi"),
                    right: text("right"),
                    style: text("style"),
                    strike_lo: num("strike_lo"),
                    strike_hi: num("strike_hi"),
                    min_open_interest: f.get("min_open_interest").and_then(Value::as_i64),
                })),
                spot: body.get("spot").and_then(Value::as_f64),
            }))
        }
        "catalog" => QueryForm::Catalog(Box::new(Catalog {
            relation: relation()?,
            predicate: opt_predicate(body)?,
        })),
        other => Err(Malformed(format!("`{}` is not a form: point, latest, series, section, scan, exists, count, distinct or chain", other)))?,
    })
}

/// The relation a form reads.
pub fn relation_of(f: &QueryForm) -> &str {
    match f {
        QueryForm::Point(p) => &p.relation,
        QueryForm::Latest(p) => &p.relation,
        QueryForm::Series(p) => &p.relation,
        QueryForm::Section(p) => &p.relation,
        QueryForm::Scan(p) => &p.relation,
        QueryForm::Exists(p) => &p.relation,
        QueryForm::CountRows(p) => &p.relation,
        QueryForm::Distinct(p) => &p.relation,
        QueryForm::Catalog(p) => &p.relation,
        QueryForm::Chain(_) => "chain_index",
    }
}

/// The form's kind, for the audit.
pub fn kind_of(f: &QueryForm) -> &'static str {
    match f {
        QueryForm::Point(_) => "point",
        QueryForm::Latest(_) => "latest",
        QueryForm::Series(_) => "series",
        QueryForm::Section(_) => "section",
        QueryForm::Scan(_) => "scan",
        QueryForm::Exists(_) => "exists",
        QueryForm::CountRows(_) => "count",
        QueryForm::Distinct(_) => "distinct",
        QueryForm::Catalog(_) => "catalog",
        QueryForm::Chain(_) => "chain",
    }
}

/// A request of the current versions, unclamped: the gateway's own reads
/// (provenance, the registry) and the forward scheduler's.
pub fn request(form: QueryForm) -> QueryRequest {
    QueryRequest {
        view: Some(Box::new(View {
            versions: None,
            binding: None,
            as_of_seq: None,
        })),
        form,
        receipt_filter: None,
        with_seq: false,
        explain: false,
        ticks: false,
    }
}

/// The clamp (PL-7.1, Proposition PL-4.2): `r_max := min(request, h)` and
/// `as_of_seq := seq(h)` when known, whatever the request asked.
pub fn clamp(req: &mut QueryRequest, h: i64, seq_h: Option<u64>) {
    let asked = req.receipt_filter.as_ref().and_then(|f| f.r_max);
    let r_max = asked.map_or(h, |r| r.min(h));
    req.receipt_filter = Some(Box::new(ReceiptFilter { r_max: Some(r_max) }));
    let mut view = req.view.take().map(|v| *v).unwrap_or(View {
        versions: None,
        binding: None,
        as_of_seq: None,
    });
    view.as_of_seq = match (view.as_of_seq, seq_h) {
        (Some(a), Some(s)) => Some(a.min(s)),
        (None, s) => s,
        (a, None) => a,
    };
    req.view = Some(Box::new(view));
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn forms_render_from_json() {
        let f = form(&json!({"kind": "series", "relation": "equity.close", "identity": [7], "t0": "2024-01-01", "t1": "2024-02-01"})).unwrap();
        match f {
            QueryForm::Series(s) => {
                assert_eq!(s.relation, "equity.close");
                assert_eq!(s.identity.unwrap().values[0].int, 7);
                assert_eq!(s.t0, abt_wire::scalar::parse_timestamp("2024-01-01").unwrap());
            }
            _ => panic!(),
        }
        let f = form(&json!({"kind": "count", "relation": "equity.close", "predicate": "P > 10 and A in (1, 2) or S = ES"})).unwrap();
        match f {
            QueryForm::CountRows(c) => {
                let p = c.predicate.unwrap();
                let all = p.all.unwrap();
                assert_eq!(all.len(), 2);
                assert_eq!(all[0].atoms[0].op, Op::Gt);
                assert_eq!(all[1].atoms.len(), 2);
                assert_eq!(all[1].atoms[0].op, Op::In);
                assert_eq!(all[1].atoms[1].values.as_ref().unwrap()[0].text.as_deref(), Some("ES"));
                assert_eq!(c.t0, i64::MIN);
            }
            _ => panic!(),
        }
        assert!(form(&json!({"kind": "nope", "relation": "x"})).is_err());
        assert!(form(&json!({"kind": "point", "relation": "x"})).is_err());
        assert_eq!(constant("2024-03-05").kind, ConstantKind::Time);
        assert_eq!(constant("3.5").kind, ConstantKind::Num);
        assert_eq!(constant("true").kind, ConstantKind::Bool);
    }

    #[test]
    fn the_clamp_never_widens() {
        let h = 100;
        let mut r = request(form(&json!({"kind": "section", "relation": "r", "t": 5})).unwrap());
        clamp(&mut r, h, Some(9));
        assert_eq!(r.receipt_filter.as_ref().unwrap().r_max, Some(100));
        assert_eq!(r.view.as_ref().unwrap().as_of_seq, Some(9));
        r.receipt_filter = Some(Box::new(ReceiptFilter { r_max: Some(1_000) }));
        r.view.as_mut().unwrap().as_of_seq = Some(1_000);
        clamp(&mut r, h, Some(9));
        assert_eq!(r.receipt_filter.as_ref().unwrap().r_max, Some(100));
        assert_eq!(r.view.as_ref().unwrap().as_of_seq, Some(9));
        r.receipt_filter = Some(Box::new(ReceiptFilter { r_max: Some(50) }));
        clamp(&mut r, h, None);
        assert_eq!(r.receipt_filter.as_ref().unwrap().r_max, Some(50));
    }
}
