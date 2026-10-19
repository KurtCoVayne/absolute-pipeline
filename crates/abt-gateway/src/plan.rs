//! The plan guards (PL-6.4, PL-6.6, contracts section 5), applied to a
//! plan's text before the query layer sees it. A small tolerant reader of
//! the plan grammar (Q-2.1) takes the lines the guards read: `program`,
//! `holdout`, `window`, `budget`, `grid`, `steps`; and of a program's text,
//! its strategy's name and `execution` blocks. Everything else is the query
//! layer's to parse and refuse.

use std::collections::BTreeMap;

/// A refusal by a guard: `409` with its code (contracts section 5).
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{code}: {message}")]
pub struct Refusal {
    pub code: &'static str,
    pub message: String,
}

fn refuse(code: &'static str, message: impl Into<String>) -> Refusal {
    Refusal { code, message: message.into() }
}

/// Which hold-out a plan declares (PL-6.6): the development plan's,
/// trailing over a window ending at `H`; or the forward plan's, after `H`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Development,
    Forward,
}

/// A run step's own window, as written, and its observations.
pub type RunStep = (Option<(String, String)>, Vec<String>);

/// What the guards read of a study.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Study {
    pub name: String,
    /// `program`'s value as written: a path (`slug.dsl`) or a name.
    pub program: String,
    pub holdout: Option<String>,
    pub window: Option<(String, String)>,
    /// `budget`'s entries, each `key value`.
    pub budget: Option<Vec<String>>,
    pub grid: Vec<(String, Vec<String>)>,
    /// The `steps` block's entries as written, when the block is given.
    pub steps: Option<Vec<String>>,
    pub claims: Vec<String>,
}

impl Study {
    /// The number of points of the grid (the Cartesian product of its
    /// axes; one point without a grid), what `grid_points` bounds.
    pub fn grid_size(&self) -> u64 {
        self.grid.iter().map(|(_, v)| v.len() as u64).product::<u64>().max(1)
    }

    /// The steps as the query layer takes them: the given block, else the
    /// default steps (a grid when there is one, else one run; then the
    /// report).
    pub fn step_lines(&self) -> Vec<String> {
        match &self.steps {
            Some(s) => s.clone(),
            None => vec![if self.grid.is_empty() { "run main".to_string() } else { "grid main".to_string() }, "report report".to_string()],
        }
    }

    /// The names of the `reveal` steps.
    pub fn reveal_steps(&self) -> Vec<String> {
        self.step_lines()
            .iter()
            .filter_map(|l| l.strip_prefix("reveal ").map(|r| r.split_whitespace().next().unwrap_or("").to_string()))
            .collect()
    }

    /// A `run` step's own window (`over A .. B`) and observations
    /// (`observe R, …`), as written; `None` for any other step.
    pub fn run_step(&self, name: &str) -> Option<RunStep> {
        let line = self
            .step_lines()
            .into_iter()
            .find(|l| l.strip_prefix("run ").is_some_and(|r| r.split_whitespace().next() == Some(name)))?;
        let rest = &line["run ".len()..];
        let over = rest.find(" over ").map(|i| {
            let w = &rest[i + " over ".len()..];
            let w = w.split(" observe ").next().unwrap_or(w);
            let (a, b) = w.split_once("..").unwrap_or((w, ""));
            (a.trim().to_string(), b.trim().to_string())
        });
        let observe = rest
            .find(" observe ")
            .map(|i| rest[i + " observe ".len()..].split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect())
            .unwrap_or_default();
        Some((over, observe))
    }

    /// The name of the first `run` step, what a forward reveal reveals.
    pub fn first_run_step(&self) -> Option<String> {
        self.step_lines()
            .iter()
            .find_map(|l| l.strip_prefix("run ").map(|r| r.split_whitespace().next().unwrap_or("").to_string()))
    }

    /// How many kernel runs the plan asks for at most: its grid's points
    /// per grid step, one per run step, as `plan check` counts them.
    pub fn runs_asked(&self) -> u64 {
        self.step_lines()
            .iter()
            .map(|l| match l.split_whitespace().next() {
                Some("grid") => self.grid_size(),
                Some("run") => 1,
                _ => 0,
            })
            .sum()
    }
}

/// The text without its comments: `#` to the end of the line, outside a
/// quoted string.
pub fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let mut quoted = false;
        let mut cut = line.len();
        for (i, c) in line.char_indices() {
            match c {
                '"' => quoted = !quoted,
                '#' if !quoted => {
                    cut = i;
                    break;
                }
                _ => {}
            }
        }
        out.push_str(&line[..cut]);
        out.push('\n');
    }
    out
}

/// The index just after the brace matching the one at `open`.
fn matching_brace(text: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut quoted = false;
    for (i, c) in text[open..].char_indices() {
        match c {
            '"' => quoted = !quoted,
            '{' if !quoted => depth += 1,
            '}' if !quoted => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(open + i + 1);
                }
            }
            _ => {}
        }
    }
    None
}

/// `text` split at every `sep` outside parentheses, braces, brackets and
/// quotes.
fn split_top(text: &str, seps: &[char]) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut quoted = false;
    let mut cur = String::new();
    for c in text.chars() {
        match c {
            '"' => quoted = !quoted,
            '(' | '{' | '[' if !quoted => depth += 1,
            ')' | '}' | ']' if !quoted => depth -= 1,
            _ => {}
        }
        if depth == 0 && !quoted && seps.contains(&c) {
            out.push(std::mem::take(&mut cur));
        } else {
            cur.push(c);
        }
    }
    out.push(cur);
    out
}

/// A block's entries: separated by `;` or new lines, whitespace collapsed.
fn entries(block: &str) -> Vec<String> {
    split_top(block, &['\n', ';'])
        .iter()
        .map(|e| e.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|e| !e.is_empty())
        .collect()
}

/// A unit of a plan file.
#[derive(Clone, Debug)]
enum Unit {
    Study { name: String, body: String },
    Template { name: String, params: Vec<String>, body: String },
    Instance { name: String, template: String, args: Vec<(String, String)> },
}

fn units(text: &str) -> Result<Vec<Unit>, String> {
    let text = strip_comments(text);
    let mut out = Vec::new();
    let mut rest = text.as_str();
    loop {
        rest = rest.trim_start();
        if rest.is_empty() {
            return Ok(out);
        }
        let (kw, after) = rest.split_once(char::is_whitespace).ok_or_else(|| format!("`{}` is not a unit: study or template", rest.trim()))?;
        let after = after.trim_start();
        match kw {
            "study" => {
                let name_end = after.find(|c: char| !(c.is_alphanumeric() || c == '_')).unwrap_or(after.len());
                let name = after[..name_end].to_string();
                let tail = after[name_end..].trim_start();
                if let Some(inst) = tail.strip_prefix('=') {
                    let inst = inst.trim_start();
                    let open = inst.find('(').ok_or_else(|| format!("study {}: a template's arguments are in parentheses", name))?;
                    let close = inst.find(')').ok_or_else(|| format!("study {}: the arguments are not closed", name))?;
                    let template = inst[..open].trim().to_string();
                    let mut args = Vec::new();
                    for a in inst[open + 1..close].split(',').map(str::trim).filter(|a| !a.is_empty()) {
                        let (k, v) = a.split_once('=').ok_or_else(|| format!("study {}: `{}` is not PARAM = value", name, a))?;
                        args.push((k.trim().to_string(), v.trim().to_string()));
                    }
                    out.push(Unit::Instance { name, template, args });
                    rest = &inst[close + 1..];
                } else if tail.starts_with('{') {
                    let offset = text.len() - tail.len();
                    let end = matching_brace(&text, offset).ok_or_else(|| format!("study {}: its body is not closed", name))?;
                    out.push(Unit::Study {
                        name,
                        body: text[offset + 1..end - 1].to_string(),
                    });
                    rest = &text[end..];
                } else {
                    return Err(format!("study {}: a body in braces or `= TEMPLATE(...)` follows the name", name));
                }
            }
            "template" => {
                let open = after.find('(').ok_or("a template declares its parameters in parentheses")?;
                let close = after.find(')').ok_or("a template's parameters are not closed")?;
                let name = after[..open].trim().to_string();
                let params = after[open + 1..close].split(',').map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect();
                let tail = after[close + 1..].trim_start();
                if !tail.starts_with('{') {
                    return Err(format!("template {}: its body follows in braces", name));
                }
                let offset = text.len() - tail.len();
                let end = matching_brace(&text, offset).ok_or_else(|| format!("template {}: its body is not closed", name))?;
                out.push(Unit::Template {
                    name,
                    params,
                    body: text[offset + 1..end - 1].to_string(),
                });
                rest = &text[end..];
            }
            other => return Err(format!("`{}` begins no unit: a plan file holds studies and templates", other)),
        }
    }
}

/// The body with every parameter, as a whole word, replaced by its value.
fn substitute(body: &str, args: &[(String, String)]) -> String {
    let mut out = String::with_capacity(body.len());
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut String| {
        match args.iter().find(|(k, _)| k == word) {
            Some((_, v)) => out.push_str(v),
            None => out.push_str(word),
        }
        word.clear();
    };
    for c in body.chars() {
        if c.is_alphanumeric() || c == '_' {
            word.push(c);
        } else {
            flush(&mut word, &mut out);
            out.push(c);
        }
    }
    flush(&mut word, &mut out);
    out
}

fn study(name: &str, body: &str) -> Result<Study, String> {
    let mut s = Study {
        name: name.to_string(),
        ..Default::default()
    };
    let mut i = 0;
    while i < body.len() {
        let rest = &body[i..];
        let trimmed = rest.trim_start();
        if trimmed.is_empty() {
            break;
        }
        i += rest.len() - trimmed.len();
        let kw_end = trimmed.find(|c: char| c.is_whitespace() || c == '{').unwrap_or(trimmed.len());
        let kw = &trimmed[..kw_end];
        let line_end = trimmed.find('\n').unwrap_or(trimmed.len());
        let value = trimmed[kw_end..line_end].trim().to_string();
        match kw {
            "program" | "holdout" | "walk_forward" | "window" | "objective" => {
                match kw {
                    "program" => s.program = value,
                    "holdout" => s.holdout = Some(value),
                    "window" => {
                        let (a, b) = value.split_once("..").ok_or_else(|| format!("study {}: `window {}` is not DATE .. DATE", name, value))?;
                        s.window = Some((a.trim().to_string(), b.trim().to_string()));
                    }
                    _ => {}
                }
                i += line_end;
            }
            "params" | "grid" | "budget" | "claims" | "steps" | "report" => {
                let open = body[i..].find('{').map(|o| i + o).ok_or_else(|| format!("study {}: `{}` takes a block in braces", name, kw))?;
                let end = matching_brace(body, open).ok_or_else(|| format!("study {}: the `{}` block is not closed", name, kw))?;
                let block = &body[open + 1..end - 1];
                match kw {
                    "grid" => {
                        for e in split_top(block, &['\n', ';']) {
                            let e = e.split_whitespace().collect::<Vec<_>>().join(" ");
                            if e.is_empty() {
                                continue;
                            }
                            let (k, v) = e.split_once('=').ok_or_else(|| format!("study {}: grid `{}` is not NAME = v1, v2", name, e))?;
                            let values: Vec<String> = v.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect();
                            s.grid.push((k.trim().to_string(), values));
                        }
                    }
                    "budget" => s.budget = Some(entries(block)),
                    "steps" => s.steps = Some(entries(block)),
                    "claims" => s.claims = entries(block),
                    _ => {}
                }
                i = end;
            }
            other => return Err(format!("study {}: `{}` is not a field of a study", name, other)),
        }
    }
    if s.program.is_empty() {
        return Err(format!("study {} names no program", name));
    }
    Ok(s)
}

/// The one study of a plan's text (`--study` is not offered: a plan the
/// agent sends holds one), templates substituted.
pub fn parse(text: &str) -> Result<Study, String> {
    let us = units(text)?;
    let studies: Vec<&Unit> = us.iter().filter(|u| !matches!(u, Unit::Template { .. })).collect();
    let unit = match studies.as_slice() {
        [one] => *one,
        [] => return Err("the plan holds no study".into()),
        many => {
            return Err(format!(
                "the plan holds several studies ({}): send one",
                many.iter().map(|u| unit_name(u)).collect::<Vec<_>>().join(", ")
            ))
        }
    };
    match unit {
        Unit::Study { name, body } => study(name, body),
        Unit::Instance { name, template, args } => {
            let Some(Unit::Template { params, body, .. }) = us.iter().find(|u| matches!(u, Unit::Template { name: n, .. } if n == template)) else {
                return Err(format!("study {} instantiates no template `{}`", name, template));
            };
            if let Some(missing) = params.iter().find(|p| !args.iter().any(|(k, _)| k == *p)) {
                return Err(format!("study {}: template {}'s parameter `{}` has no value", name, template, missing));
            }
            study(name, &substitute(body, args))
        }
        Unit::Template { .. } => unreachable!("templates are not studies"),
    }
}

fn unit_name(u: &Unit) -> &str {
    match u {
        Unit::Study { name, .. } | Unit::Template { name, .. } | Unit::Instance { name, .. } => name,
    }
}

/// `H` as the plan writes it (contracts section 5): `YYYY-MM-DDTHH:MM:SSZ`.
pub fn render_h(h: i64) -> String {
    pipeline_wire::iso(h - h.rem_euclid(pipeline_wire::NS_PER_S))
}

/// Whether a time written in a plan is `H` (to the second, which is how
/// the plan writes it).
fn is_h(written: &str, h: i64) -> bool {
    abt_wire::scalar::parse_timestamp(written).is_some_and(|t| t.div_euclid(pipeline_wire::NS_PER_S) == h.div_euclid(pipeline_wire::NS_PER_S))
}

/// The hold-out guard (PL-6.6; contracts section 5): `trailing:…` with a
/// `window … .. H`, or `after H`; and the budget guard: `budget { money 0;
/// … }`.
pub fn kind_of(s: &Study, h: i64) -> Result<Kind, Refusal> {
    let hs = render_h(h);
    let kind = match s.holdout.as_deref().map(str::trim) {
        Some(ho) if ho.starts_with("trailing:") => match &s.window {
            Some((_, to)) if is_h(to, h) => Kind::Development,
            Some((_, to)) => return Err(refuse("holdout", format!("study {}: the development plan's window ends at `{}`, not at H ({})", s.name, to, hs))),
            None => return Err(refuse("holdout", format!("study {}: a trailing hold-out needs `window FIRST .. {}`", s.name, hs))),
        },
        Some(ho) if ho.starts_with("after ") || ho.starts_with("after\t") => {
            let at = ho["after".len()..].trim();
            if !is_h(at, h) {
                return Err(refuse("holdout", format!("study {}: `holdout after {}` is not after H ({})", s.name, at, hs)));
            }
            Kind::Forward
        }
        Some(other) => {
            return Err(refuse(
                "holdout",
                format!("study {}: `holdout {}` is neither `trailing:…` over a window ending at H nor `after {}`", s.name, other, hs),
            ))
        }
        None => {
            return Err(refuse(
                "holdout",
                format!("study {}: a plan declares its hold-out: `trailing:…` with `window FIRST .. {}`, or `after {}`", s.name, hs, hs),
            ))
        }
    };
    let money_zero = s.budget.as_ref().is_some_and(|b| {
        b.iter().any(|e| {
            e.split_whitespace().next() == Some("money")
                && e.split_whitespace()
                    .nth(1)
                    .is_some_and(|v| v.trim_start_matches('0').trim_start_matches('.').trim_start_matches('0').trim().is_empty() || v == "0")
        })
    });
    if !money_zero {
        return Err(refuse(
            "budget",
            format!("study {}: `budget {{ money 0; … }}` is required (A-D22: the pipeline spends nothing on data)", s.name),
        ));
    }
    Ok(kind)
}

/// The grid guard: the plan's points within what the job has left.
pub fn grid_guard(s: &Study, left: u64) -> Result<u64, Refusal> {
    let n = s.grid_size();
    if n > left {
        return Err(refuse("grid", format!("study {}: the grid has {} points; the job has {} left of its grid_points", s.name, n, left)));
    }
    Ok(n)
}

/// The reveal guard: an agent reveals no forward plan (PL-6.4).
pub fn reveal_guard(s: &Study, kind: Kind) -> Result<(), Refusal> {
    if kind == Kind::Forward {
        let r = s.reveal_steps();
        if !r.is_empty() {
            return Err(refuse(
                "reveal",
                format!("study {}: the forward plan's reveal ({}) is the scheduler's, never an agent's", s.name, r.join(", ")),
            ));
        }
    }
    Ok(())
}

/// The program a study names among the programs the request carries:
/// `slug.dsl` by key or basename, or a strategy by name; the strategy's
/// name and text.
pub fn program_of<'a>(s: &Study, programs: &'a BTreeMap<String, String>) -> Result<(String, &'a str), Refusal> {
    let want = s.program.trim();
    let by_key = programs.get(want).map(|t| (want, t)).or_else(|| {
        let base = want.rsplit('/').next().unwrap_or(want);
        programs.iter().find(|(k, _)| k.rsplit('/').next().unwrap_or(k) == base).map(|(k, t)| (k.as_str(), t))
    });
    let (_, text) = match by_key {
        Some(x) => x,
        None => {
            let name = want.trim_end_matches(".dsl");
            programs.iter().find(|(_, t)| strategy_name(t).as_deref() == Some(name)).map(|(k, t)| (k.as_str(), t)).ok_or_else(|| {
                refuse(
                    "lineage",
                    format!(
                        "study {}: `program {}` names none of the programs sent ({})",
                        s.name,
                        want,
                        programs.keys().cloned().collect::<Vec<_>>().join(", ")
                    ),
                )
            })?
        }
    };
    let name = strategy_name(text).ok_or_else(|| refuse("lineage", format!("study {}: the program `{}` declares no strategy", s.name, want)))?;
    Ok((name, text.as_str()))
}

/// The first strategy a source declares.
pub fn strategy_name(text: &str) -> Option<String> {
    strip_comments(text)
        .lines()
        .find_map(|l| {
            l.trim()
                .strip_prefix("strategy ")
                .map(|r| r.split(|c: char| c.is_whitespace() || c == '{').next().unwrap_or("").to_string())
        })
        .filter(|s| !s.is_empty())
}

/// The house presets (guideline section 15).
pub const HOUSE: [&str; 2] = ["futures_house", "equities_house"];

/// The preset guard (PL-6.4): every `execution` of every program names a
/// house preset, or one of them `with { one field }` in a strategy named
/// for that field; no inline block, no preset defined by the agent.
pub fn preset_guard(programs: &BTreeMap<String, String>) -> Result<(), Refusal> {
    for (file, text) in programs {
        let clean = strip_comments(text);
        let strategy = strategy_name(&clean).unwrap_or_default();
        let mut from = 0;
        while let Some(i) = clean[from..].find("execution") {
            let at = from + i;
            from = at + "execution".len();
            // A word of its own.
            let before = clean[..at].chars().next_back();
            let after = clean[from..].chars().next();
            if before.is_some_and(|c| c.is_alphanumeric() || c == '_') || !after.is_some_and(|c| c.is_whitespace() || c == '{') {
                continue;
            }
            let rest = clean[from..].trim_start();
            if rest.starts_with('{') {
                return Err(refuse(
                    "preset",
                    format!("{}: `execution {{ … }}` is written out; name a house preset ({}) instead", file, HOUSE.join(" or ")),
                ));
            }
            let name_end = rest.find(|c: char| !(c.is_alphanumeric() || c == '_')).unwrap_or(rest.len());
            let name = &rest[..name_end];
            let tail = rest[name_end..].trim_start();
            if !HOUSE.contains(&name) {
                return Err(refuse("preset", format!("{}: `execution {}` is not a house preset ({})", file, name, HOUSE.join(" or "))));
            }
            if tail.starts_with('{') {
                return Err(refuse(
                    "preset",
                    format!("{}: `execution {} {{ … }}` defines a preset; the house presets are the library's", file, name),
                ));
            }
            if let Some(w) = tail.strip_prefix("with") {
                let w = w.trim_start();
                let Some(end) = w.starts_with('{').then(|| matching_brace(w, 0)).flatten() else {
                    return Err(refuse("preset", format!("{}: `execution {} with` takes one field in braces", file, name)));
                };
                let fields = entries(&w[1..end - 1]);
                if fields.len() != 1 {
                    return Err(refuse(
                        "preset",
                        format!("{}: `execution {} with {{ … }}` changes {} fields; a variant changes one", file, name, fields.len()),
                    ));
                }
                let field = fields[0].split_whitespace().next().unwrap_or("");
                let words: Vec<&str> = fields[0].split_whitespace().collect();
                let named_for = strategy.contains(field)
                    || words.iter().skip(1).any(|v| v.chars().all(|c| c.is_alphanumeric() || c == '_') && strategy.contains(v))
                    || (matches!(field, "commission" | "slippage" | "impact" | "financing" | "participation") && strategy.contains("cost"))
                    || (field == "orders" && (strategy.contains("moo") || strategy.contains("moc")));
                if !named_for {
                    return Err(refuse(
                        "preset",
                        format!("{}: strategy `{}` changes `{}` of {}; a variant is named for the field it changes", file, strategy, field, name),
                    ));
                }
            }
        }
    }
    Ok(())
}

/// `holdout after H` rewritten as the pinned query layer's grammar allows,
/// `trailing:Ny` (years), until the amendment of PL-6.6 lands: the
/// nearest the pinned tree can say. Logged by the caller.
pub fn rewrite_after_as_trailing(text: &str, years: u32) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let t = line.trim_start();
        if t.starts_with("holdout") && t["holdout".len()..].trim_start().starts_with("after") {
            let indent = &line[..line.len() - t.len()];
            out.push_str(&format!("{}holdout trailing:{}y\n", indent, years.max(1)));
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// The plan with a reveal step of `run_step` added when it has none (the
/// forward plan as the scheduler reveals it): `reveal forward of RUN` as
/// the last entry of the `steps` block, or a `steps` block with the
/// default steps and it.
pub fn with_forward_reveal(text: &str, s: &Study) -> String {
    if !s.reveal_steps().is_empty() {
        return text.to_string();
    }
    let Some(run) = s.first_run_step() else { return text.to_string() };
    let reveal = format!("reveal forward of {}", run);
    if s.steps.is_some() {
        if let Some(i) = text.find("steps") {
            if let Some(open) = text[i..].find('{').map(|o| i + o) {
                if let Some(end) = matching_brace(text, open) {
                    let mut out = text[..end - 1].trim_end().to_string();
                    out.push_str(&format!("\n    {}\n  }}", reveal));
                    out.push_str(&text[end..]);
                    return out;
                }
            }
        }
        return text.to_string();
    }
    // No steps block: add one before the study's closing brace.
    let mut lines: Vec<String> = s.step_lines();
    lines.push(reveal);
    let block = format!("  steps {{\n{}\n  }}\n", lines.iter().map(|l| format!("    {}", l)).collect::<Vec<_>>().join("\n"));
    match text.trim_end().rfind('}') {
        Some(close) => format!("{}{}{}", &text[..close], block, &text[close..]),
        None => text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: i64 = 1_700_000_000_000_000_000; // 2023-11-14T22:13:20Z

    fn dev(window_to: &str) -> String {
        format!("study idea {{\n  program idea.dsl\n  grid {{ a = 1, 2; b = 3, 4, 5 }}\n  holdout trailing:1y\n  window 2020-01-01 .. {}\n  budget {{ money 0; bytes 2G }}\n  steps {{ run base; grid sens; reveal oos of base; report main }}\n}}\n", window_to)
    }

    #[test]
    fn the_reader_takes_what_the_guards_read() {
        let s = parse(&dev("2023-11-14T22:13:20Z")).unwrap();
        assert_eq!(s.program, "idea.dsl");
        assert_eq!(s.grid_size(), 6);
        assert_eq!(s.reveal_steps(), vec!["oos"]);
        assert_eq!(s.first_run_step().as_deref(), Some("base"));
        assert_eq!(s.runs_asked(), 7);
        let t = "study s { program p\n steps { run check at { only = ES } over 2020-01-01 .. 2020-02-01 observe trades, atr\n run base; grid g } }";
        let s2 = parse(t).unwrap();
        assert_eq!(s2.run_step("check"), Some((Some(("2020-01-01".into(), "2020-02-01".into())), vec!["trades".into(), "atr".into()])));
        assert_eq!(s2.run_step("base"), Some((None, vec![])));
        assert_eq!(s2.run_step("g"), None);
        assert_eq!(kind_of(&s, H), Ok(Kind::Development));
        let t = "template t(X) { program p\n holdout after X\n budget { money 0 } }\nstudy f = t(X = 2023-11-14T22:13:20Z)";
        let s = parse(t).unwrap();
        assert_eq!(kind_of(&s, H), Ok(Kind::Forward));
        assert_eq!(s.step_lines(), vec!["run main", "report report"]);
        assert!(parse("study a { program p }\nstudy b { program p }").is_err());
        assert!(parse("nonsense").is_err());
    }

    #[test]
    fn the_holdout_and_budget_guards_refuse_with_their_codes() {
        assert_eq!(kind_of(&parse(&dev("2023-11-13")).unwrap(), H).unwrap_err().code, "holdout");
        let no_window = dev("x").replace("  window 2020-01-01 .. x\n", "");
        assert_eq!(kind_of(&parse(&no_window).unwrap(), H).unwrap_err().code, "holdout");
        let none = dev("2023-11-14T22:13:20Z").replace("holdout trailing:1y", "holdout none");
        assert_eq!(kind_of(&parse(&none).unwrap(), H).unwrap_err().code, "holdout");
        let paid = dev("2023-11-14T22:13:20Z").replace("money 0", "money 10");
        assert_eq!(kind_of(&parse(&paid).unwrap(), H).unwrap_err().code, "budget");
        let unbudgeted = dev("2023-11-14T22:13:20Z").replace("  budget { money 0; bytes 2G }\n", "");
        assert_eq!(kind_of(&parse(&unbudgeted).unwrap(), H).unwrap_err().code, "budget");
        let fwd = parse("study f { program p\n holdout after 2023-11-14T22:13:20Z\n budget { money 0 }\n steps { run base; reveal now of base } }").unwrap();
        assert_eq!(reveal_guard(&fwd, Kind::Forward).unwrap_err().code, "reveal");
        let s = parse(&dev("2023-11-14T22:13:20Z")).unwrap();
        assert!(reveal_guard(&s, Kind::Development).is_ok());
        assert_eq!(grid_guard(&s, 5).unwrap_err().code, "grid");
        assert_eq!(grid_guard(&s, 6), Ok(6));
    }

    #[test]
    fn the_preset_guard() {
        let mut p = BTreeMap::new();
        p.insert("idea.dsl".to_string(), "strategy idea {\n  execution futures_house\n}".to_string());
        assert!(preset_guard(&p).is_ok());
        p.insert("v.dsl".to_string(), "strategy idea_no_costs {\n  execution futures_house with { commission none }\n}".to_string());
        assert!(preset_guard(&p).is_ok());
        p.insert("v.dsl".to_string(), "strategy idea_moo {\n  execution futures_house with { orders moo }\n}".to_string());
        assert!(preset_guard(&p).is_ok());
        p.insert("v.dsl".to_string(), "strategy idea_two {\n  execution futures_house with { orders moo; slippage none }\n}".to_string());
        assert_eq!(preset_guard(&p).unwrap_err().code, "preset");
        p.insert("v.dsl".to_string(), "strategy idea_b {\n  execution futures_house with { lot whole }\n}".to_string());
        assert_eq!(preset_guard(&p).unwrap_err().code, "preset");
        p.insert("v.dsl".to_string(), "strategy idea {\n  execution equity_default\n}".to_string());
        assert_eq!(preset_guard(&p).unwrap_err().code, "preset");
        p.insert("v.dsl".to_string(), "strategy idea {\n  execution { price close }\n}".to_string());
        assert_eq!(preset_guard(&p).unwrap_err().code, "preset");
        p.insert("v.dsl".to_string(), "execution futures_house { price close }\nstrategy idea { execution futures_house }".to_string());
        assert_eq!(preset_guard(&p).unwrap_err().code, "preset");
        p.insert("v.dsl".to_string(), "strategy idea {\n  # execution { commented }\n  execution futures_house\n}".to_string());
        assert!(preset_guard(&p).is_ok());
    }

    #[test]
    fn programs_resolve_by_file_or_by_name() {
        let mut p = BTreeMap::new();
        p.insert("idea.dsl".to_string(), "strategy idea_v2 {\n}".to_string());
        let s = parse("study a { program idea.dsl }").unwrap();
        assert_eq!(program_of(&s, &p).unwrap().0, "idea_v2");
        let s = parse("study a { program idea_v2 }").unwrap();
        assert_eq!(program_of(&s, &p).unwrap().0, "idea_v2");
        let s = parse("study a { program other.dsl }").unwrap();
        assert_eq!(program_of(&s, &p).unwrap_err().code, "lineage");
    }

    #[test]
    fn rewrites() {
        let t = "study f {\n  program p\n  holdout after 2023-11-14T22:13:20Z\n  budget { money 0 }\n}\n";
        assert_eq!(rewrite_after_as_trailing(t, 2), "study f {\n  program p\n  holdout trailing:2y\n  budget { money 0 }\n}\n");
        let s = parse(t).unwrap();
        let with = with_forward_reveal(t, &s);
        assert!(with.contains("steps {\n    run main\n    report report\n    reveal forward of main\n  }"), "{}", with);
        assert_eq!(parse(&with).unwrap().reveal_steps(), vec!["forward"]);
        let t2 = "study f {\n  program p\n  holdout after 2023-11-14T22:13:20Z\n  budget { money 0 }\n  steps { run base; report main }\n}\n";
        let with2 = with_forward_reveal(t2, &parse(t2).unwrap());
        assert_eq!(parse(&with2).unwrap().reveal_steps(), vec!["forward"]);
        assert_eq!(parse(&with2).unwrap().first_run_step().as_deref(), Some("base"));
        assert_eq!(render_h(H), "2023-11-14T22:13:20Z");
        assert_eq!(render_h(H + 5), "2023-11-14T22:13:20Z");
    }
}
