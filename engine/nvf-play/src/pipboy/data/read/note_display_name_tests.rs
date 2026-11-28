//! Scoped regression tests for Pip-Boy DATA -> Notes display name fallback
//! (`FULL` -> `EDID` -> `FormId`) when `NOTE` records omit `FULL`.

use super::note;
use fnv_esm::{LoadOrder, Plugin, Schema};

fn z(s: &str) -> Vec<u8> {
    [s.as_bytes(), &[0]].concat()
}

fn sub(sig: &[u8; 4], data: Vec<u8>) -> Vec<u8> {
    [&sig[..], &(data.len() as u16).to_le_bytes(), &data].concat()
}

fn record(sig: &[u8; 4], id: u32, subs: &[Vec<u8>]) -> Vec<u8> {
    let body: Vec<u8> = subs.iter().flat_map(|s| s.iter().copied()).collect();
    [&sig[..], &(body.len() as u32).to_le_bytes(), &[0; 4], &id.to_le_bytes(), &[0; 8], &body].concat()
}

fn group(label: [u8; 4], body: &[u8]) -> Vec<u8> {
    [&b"GRUP"[..], &((24 + body.len()) as u32).to_le_bytes(), &label[..], &0i32.to_le_bytes(), &[0; 8], body].concat()
}

fn tes4() -> Vec<u8> {
    let hedr = [1.34f32.to_le_bytes(), 0i32.to_le_bytes(), 0x800u32.to_le_bytes()].concat();
    record(b"TES4", 0, &[sub(b"HEDR", hedr)])
}

#[test]
fn note_display_name_prefers_full_then_edid_then_form_id() {
    const WITH_FULL: u32 = 0x0000_0101;
    const EDID_ONLY: u32 = 0x0000_0102;
    const BLANK_FULL_WITH_EDID: u32 = 0x0000_0103;
    const NAMELESS: u32 = 0x0000_0104;

    let notes = [
        record(
            b"NOTE",
            WITH_FULL,
            &[
                sub(b"EDID", z("SynthNoteWithFull")),
                sub(b"FULL", z("Field Dispatch")),
                sub(b"DATA", vec![1]),
                sub(b"TNAM", z("Report body.")),
            ],
        ),
        record(
            b"NOTE",
            EDID_ONLY,
            &[
                sub(b"EDID", z("SynthTerminalLogEntry")),
                sub(b"DATA", vec![1]),
                sub(b"TNAM", z("Terminal body.")),
            ],
        ),
        record(
            b"NOTE",
            BLANK_FULL_WITH_EDID,
            &[
                sub(b"EDID", z("SynthBlankFullEntry")),
                sub(b"FULL", z("   ")),
                sub(b"DATA", vec![1]),
                sub(b"TNAM", z("Blank full body.")),
            ],
        ),
        record(
            b"NOTE",
            NAMELESS,
            &[sub(b"DATA", vec![1]), sub(b"TNAM", z("Neither FULL nor EDID."))],
        ),
    ]
    .concat();

    let plugin_bytes = [tes4(), group(*b"NOTE", &notes)].concat();
    let plugins = [Plugin::parse(&plugin_bytes, "SynthNotes.esm".into()).unwrap()];
    let lo = LoadOrder::new(&plugins).unwrap();
    let schema = Schema::load_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/fnv")).unwrap();

    let full = note(&lo, &schema, WITH_FULL).expect("note with FULL");
    assert_eq!(full.name, "Field Dispatch");
    assert_eq!(full.text.as_deref(), Some("Report body."));

    let edid = note(&lo, &schema, EDID_ONLY).expect("note with EDID only");
    assert_eq!(edid.name, "SynthTerminalLogEntry");
    assert_eq!(edid.text.as_deref(), Some("Terminal body."));

    let blank_full = note(&lo, &schema, BLANK_FULL_WITH_EDID).expect("note with whitespace FULL");
    assert_eq!(blank_full.name, "SynthBlankFullEntry");

    let nameless = note(&lo, &schema, NAMELESS).expect("note with neither FULL nor EDID");
    assert_eq!(nameless.name, "0x00000104");
}
