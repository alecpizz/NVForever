//! A NIF file written by a test (20.2.0.7, user version 11, BS version 34): for this crate's
//! unit tests and for `read_again`.

pub fn words(out: &mut Vec<u8>, values: &[u32]) {
    out.extend(values.iter().flat_map(|v| v.to_le_bytes()));
}

pub fn floats(out: &mut Vec<u8>, values: &[f32]) {
    out.extend(values.iter().flat_map(|v| v.to_le_bytes()));
}

pub fn nif(blocks: &[(&str, Vec<u8>)], strings: &[&str]) -> Vec<u8> {
    fn string(out: &mut Vec<u8>, s: &str) {
        words(out, &[s.len() as u32]);
        out.extend(s.as_bytes());
    }
    let mut out = b"Gamebryo File Format, Version 20.2.0.7\n".to_vec();
    words(&mut out, &[0x1402_0007]);
    out.push(1);
    words(&mut out, &[11, blocks.len() as u32, 34]);
    out.extend([1, 0, 1, 0, 1, 0]); // three empty export strings
    let mut types = vec![];
    for (kind, _) in blocks {
        if !types.contains(kind) {
            types.push(*kind);
        }
    }
    out.extend((types.len() as u16).to_le_bytes());
    for kind in &types {
        string(&mut out, kind);
    }
    for (kind, _) in blocks {
        out.extend((types.iter().position(|t| t == kind).unwrap() as u16).to_le_bytes());
    }
    for (_, block) in blocks {
        words(&mut out, &[block.len() as u32]);
    }
    words(&mut out, &[strings.len() as u32, strings.iter().map(|s| s.len()).max().unwrap_or(0) as u32]);
    for s in strings {
        string(&mut out, s);
    }
    words(&mut out, &[0]); // groups
    for (_, block) in blocks {
        out.extend(block);
    }
    words(&mut out, &[1, 0]); // footer: one root, block 0
    out
}
