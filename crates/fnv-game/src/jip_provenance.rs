//! Bounded registrar metadata, not callable declaration or plugin attribution guesses.
//! [design: docs/spec-notes/jip-registration-provenance.md]
use crate::{data::Compiled, game::RunCtx};
use fnv_script::UnitOwner;
use std::{collections::BTreeMap, sync::Arc};

pub(crate) const TEXT_CHARS: usize = 192;
pub(crate) const COMPILED_ORIGINS: usize = 128;

#[derive(Debug, Clone)]
pub(crate) struct Text {
    value: Arc<str>,
    truncated: bool,
}

impl Text {
    pub fn new(value: &str) -> Self {
        let mut chars = value.chars();
        let text: String = chars.by_ref().take(TEXT_CHARS).collect();
        Self { value: Arc::from(text), truncated: chars.next().is_some() }
    }

    fn diagnostic(&self) -> String {
        let mut text = self.value.chars().flat_map(char::escape_debug).collect::<String>();
        if self.truncated { text.push_str("<truncated>"); }
        text
    }

    /// Single-line label for human-readable error units: escapes control characters
    /// while keeping path separators unescaped.
    pub(crate) fn single_line(&self) -> String {
        let mut text = String::with_capacity(self.value.len());
        for c in self.value.chars() {
            if c.is_control() {
                text.extend(c.escape_debug());
            } else {
                text.push(c);
            }
        }
        if self.truncated { text.push_str("<truncated>"); }
        text
    }
}

#[derive(Debug, Clone)]
pub(crate) enum CompiledOrigin {
    File(Text),
    Dynamic,
}

#[derive(Default)]
pub(crate) struct CompiledOrigins(BTreeMap<u32, CompiledOrigin>);

impl CompiledOrigins {
    pub fn set(&mut self, id: u32, origin: CompiledOrigin) {
        self.0.insert(id, origin);
        if self.0.len() > COMPILED_ORIGINS { let _ = self.0.pop_first(); }
    }

    pub fn get(&self, id: u32) -> Option<&CompiledOrigin> { self.0.get(&id) }
}

#[derive(Debug)]
pub(crate) struct Registrar {
    kind: &'static str,
    unit: Text,
    plugin: Option<Text>,
    source: Option<Text>,
    source_status: &'static str,
    compiled: Option<u32>,
    owner: u32,
    ctx: RunCtx,
}

pub(crate) struct Diagnostic {
    pub kind: &'static str,
    pub unit: String,
    pub plugin: String,
    pub source: String,
    pub source_status: &'static str,
    pub compiled: u32,
    pub owner: u32,
    pub self_ref: u32,
    pub action_ref: u32,
    pub quest: u32,
}

impl Registrar {
    pub fn capture(unit: &Compiled, ctx: RunCtx, plugin: Option<String>, compiled: Option<u32>, origin: Option<&CompiledOrigin>) -> Self {
        let label = unit.unit.plugin.as_str();
        let (kind, source, source_status) = if compiled.is_some() {
            match origin {
                Some(CompiledOrigin::File(file)) => ("compiled_file", Some(file.clone()), "file"),
                Some(CompiledOrigin::Dynamic) => ("compiled_source", None, "no_file"),
                None => ("compiled_unavailable", None, "not_retained"),
            }
        } else if matches!(&unit.unit.owner, UnitOwner::Other { sig, .. } if sig == "SR") {
            let file = label.strip_prefix("ScriptRunner\\").map(Text::new);
            let status = if file.is_some() { "unit_label" } else { "unavailable" };
            ("script_runner", file, status)
        } else if label == "query" {
            ("query", None, "no_file")
        } else if label == "console" {
            ("console", None, "no_file")
        } else {
            ("record_unit", None, "no_file")
        };
        Self { kind, unit: Text::new(unit.shared_key()), plugin: plugin.as_deref().map(Text::new), source, source_status,
            compiled, owner: unit.unit.owner.form(), ctx }
    }

    /// Called only after the existing original error gate admits a diagnostic.
    pub fn diagnostic(&self) -> Diagnostic {
        Diagnostic { kind: self.kind, unit: self.unit.diagnostic(),
            plugin: self.plugin.as_ref().map_or_else(|| "<none>".into(), Text::diagnostic),
            source: self.source.as_ref().map_or_else(|| "<unavailable>".into(), Text::diagnostic),
            source_status: self.source_status, compiled: self.compiled.unwrap_or(0), owner: self.owner,
            self_ref: self.ctx.self_ref.map_or(0, |f| f.0), action_ref: self.ctx.action_ref.map_or(0, |f| f.0), quest: self.ctx.quest.map_or(0, |f| f.0) }
    }
}
