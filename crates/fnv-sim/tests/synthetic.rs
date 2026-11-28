//! Layer 4 behaviour on a synthetic plugin built in memory and decoded through the
//! repository schemas (no game data).

use fnv_esm::raw::{write_subrecords, Subrecord};
use fnv_esm::{LoadOrder, Plugin, Schema};
use fnv_sim::actions::{self, Script};
use fnv_sim::forms::{BaseData, Entry, ItemKind};
use fnv_sim::world::{Outcome, SimError};
use fnv_sim::{leveled, FormId, FormStore, RefTable, SimConfig, World};
use rand_chacha::ChaCha8Rng;
use rand_core::SeedableRng;

// ---- plugin builders ------------------------------------------------------------

fn sub(sig: &[u8; 4], data: Vec<u8>) -> Subrecord {
    Subrecord { sig: *sig, data, extended: false }
}

fn z(s: &str) -> Vec<u8> {
    let mut v = s.as_bytes().to_vec();
    v.push(0);
    v
}

fn cat(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}

fn record(sig: &[u8; 4], flags: u32, fid: u32, subs: &[Subrecord]) -> Vec<u8> {
    let payload = write_subrecords(subs);
    cat(&[
        sig,
        &(payload.len() as u32).to_le_bytes(),
        &flags.to_le_bytes(),
        &fid.to_le_bytes(),
        &[0; 4],
        &15u16.to_le_bytes(),
        &[0; 2],
        &payload,
    ])
}

fn group(label: [u8; 4], gtype: i32, body: &[u8]) -> Vec<u8> {
    cat(&[b"GRUP", &((24 + body.len()) as u32).to_le_bytes(), &label, &gtype.to_le_bytes(), &[0; 8], body])
}

fn top(sig: &[u8; 4], body: &[u8]) -> Vec<u8> {
    group(*sig, 0, body)
}

fn tes4() -> Vec<u8> {
    let hedr = cat(&[&1.34f32.to_le_bytes(), &0i32.to_le_bytes(), &0x800u32.to_le_bytes()]);
    record(b"TES4", 1, 0, &[sub(b"HEDR", hedr)])
}

fn value_weight(value: i32, weight: f32) -> Vec<u8> {
    cat(&[&value.to_le_bytes(), &weight.to_le_bytes()])
}

fn cnto(item: u32, count: i32) -> Subrecord {
    sub(b"CNTO", cat(&[&item.to_le_bytes(), &count.to_le_bytes()]))
}

fn lvlo(level: u16, item: u32, count: u16) -> Subrecord {
    sub(b"LVLO", cat(&[&level.to_le_bytes(), &[0; 2], &item.to_le_bytes(), &count.to_le_bytes(), &[0; 2]]))
}

fn posrot(p: [f32; 3], r: [f32; 3]) -> Vec<u8> {
    p.iter().chain(r.iter()).flat_map(|f| f.to_le_bytes()).collect()
}

fn refr(fid: u32, flags: u32, base: u32, pos: [f32; 3], extra: Vec<Subrecord>) -> Vec<u8> {
    let mut subs = vec![sub(b"NAME", base.to_le_bytes().to_vec())];
    subs.extend(extra);
    subs.push(sub(b"DATA", posrot(pos, [0.0; 3])));
    record(b"REFR", flags, fid, &subs)
}

fn xloc(level: u8, key: u32) -> Subrecord {
    sub(b"XLOC", cat(&[&[level, 0, 0, 0], &key.to_le_bytes(), &[0; 4]]))
}

fn xtel(door: u32, pos: [f32; 3]) -> Subrecord {
    sub(b"XTEL", cat(&[&door.to_le_bytes(), &posrot(pos, [0.0, 0.0, 1.5]), &0u32.to_le_bytes()]))
}

fn xesp(parent: u32, opposite: bool) -> Subrecord {
    sub(b"XESP", cat(&[&parent.to_le_bytes(), &[opposite as u8, 0, 0, 0]]))
}

fn glob_rec(fid: u32, edid: &str, ty: u8, v: f32) -> Vec<u8> {
    record(b"GLOB", 0, fid, &[sub(b"EDID", z(edid)), sub(b"FNAM", vec![ty]), sub(b"FLTV", v.to_le_bytes().to_vec())])
}

fn weap(fid: u32, edid: &str, value: i32, weight: f32) -> Vec<u8> {
    let data = cat(&[&value.to_le_bytes(), &100i32.to_le_bytes(), &weight.to_le_bytes(), &10i16.to_le_bytes(), &[12u8]]);
    record(b"WEAP", 0, fid, &[sub(b"EDID", z(edid)), sub(b"DATA", data)])
}

fn armo(fid: u32, edid: &str, slots: u32, weight: f32) -> Vec<u8> {
    armo_flags(fid, edid, slots, 0, weight)
}

/// An ARMO with BMDT general flags (0x40: Non-Playable).
fn armo_flags(fid: u32, edid: &str, slots: u32, general: u8, weight: f32) -> Vec<u8> {
    let bmdt = cat(&[&slots.to_le_bytes(), &[general, 0, 0, 0]]);
    let data = cat(&[&10i32.to_le_bytes(), &50i32.to_le_bytes(), &weight.to_le_bytes()]);
    record(b"ARMO", 0, fid, &[sub(b"EDID", z(edid)), sub(b"BMDT", bmdt), sub(b"DATA", data)])
}

fn cell_children(cell: u32, temp: &[u8], persistent: &[u8]) -> Vec<u8> {
    let label = cell.to_le_bytes();
    let mut body = Vec::new();
    if !persistent.is_empty() {
        body.extend(group(label, 8, persistent));
    }
    body.extend(group(label, 9, temp));
    group(label, 6, &body)
}

// FormIDs of the synthetic plugin (a master with no masters: top byte 00).
const STIMPAK: u32 = 0x100; // MISC, value 25, weight 0.5
const CAPS: u32 = 0x101; // MISC, value 1, weight 0
const KEY: u32 = 0x102; // KEYM
const CHAIR: u32 = 0x103; // FURN
const LL_LOOT: u32 = 0x110; // LVLI: level 1 -> STIMPAK x2, level 5 -> CAPS x10
const LL_NONE: u32 = 0x111; // LVLI: chance none 100
const LL_LOOP: u32 = 0x112; // LVLI naming itself
const LL_ODDS: u32 = 0x113; // LVLI: chance none 0, LVLG TestOdds (100), STIMPAK x1
const LL_WORN: u32 = 0x114; // LVLI: PISTOL x1 with COED condition 0.4, then STIMPAK x1 without
const ODDS: u32 = 0x151; // GLOB TestOdds = 100
const SAFE: u32 = 0x120; // CONT: CAPS x3, LL_LOOT x1, count-0 entry
const DOOR: u32 = 0x130;
const CELL_A: u32 = 0x200;
const CELL_B: u32 = 0x201;
const SAFE_REF: u32 = 0x300; // locked (level 50, KEY)
const STIM_REF: u32 = 0x301; // XCNT 3
const DOOR_A: u32 = 0x302; // -> DOOR_B
const DOOR_B: u32 = 0x303; // -> DOOR_A, in CELL_B
const PLAIN_DOOR: u32 = 0x304;
const KEY_REF: u32 = 0x305; // persistent, initially disabled
const CHAIR_REF: u32 = 0x306;
const CHILD_SAME: u32 = 0x307; // XESP -> KEY_REF (initially disabled)
const CHILD_OPPOSITE: u32 = 0x308; // XESP -> KEY_REF, opposite
const GRANDCHILD: u32 = 0x309; // XESP -> CHILD_OPPOSITE
const GUN_REF: u32 = 0x30A; // PISTOL with XHLP 0.25
const PISTOL: u32 = 0x170; // WEAP, weight 3
const RIFLE: u32 = 0x171; // WEAP, weight 8
const HELMET: u32 = 0x172; // ARMO, slots Head|Hair, weight 2
const HAT: u32 = 0x173; // ARMO, slot Hair, weight 1
const SUIT: u32 = 0x174; // ARMO, slot Upper Body, weight 15
const AID: u32 = 0x175; // ALCH, weight 0.5
const BULLETS: u32 = 0x176; // AMMO, weight 0.1 (Hardcore only)
const NOTE: u32 = 0x178; // NOTE: no weight field
const CHIP: u32 = 0x179; // CHIP: no weight field
const CMNY: u32 = 0x17A; // CMNY: no weight field
const CARD: u32 = 0x17B; // CCRD: value only
const GLOVE: u32 = 0x17C; // ARMO, slot Left Hand, Non-Playable, weight 0
const QUEST_KEY: u32 = 0x17D; // KEYM with the record header's "Quest Item" flag (bit 10)
const LOCKER: u32 = 0x121; // CONT: PISTOL x1 with COED condition 0.75
const MINE: u32 = 0x177; // WEAP firing MINE_PROJ
const MINE_PROJ: u32 = 0x180; // PROJ
const MINE_REF: u32 = 0x30B; // PGRE of MINE_PROJ
const WORLD: u32 = 0x400; // WRLD
const WORLD_PERSISTENT: u32 = 0x401; // the worldspace's persistent cell
const WORLD_CELL: u32 = 0x402; // exterior cell at grid (-18, 0)
const EXT_DOOR: u32 = 0x30C; // persistent door in WORLD_PERSISTENT
const IN_DOOR: u32 = 0x30D; // door in CELL_B -> EXT_DOOR, destination inside grid (-18, 0)
const WORLD2: u32 = 0x410; // a second WRLD, a child of WORLD (WNAM, PNAM "Use Map Data")
const WORLD2_PERSISTENT: u32 = 0x411;
const WORLD2_CELL: u32 = 0x412; // exterior cell at grid (2, 1)
const GATE_OUT: u32 = 0x30E; // persistent door in WORLD_PERSISTENT -> GATE_IN, in WORLD2
const GATE_IN: u32 = 0x30F; // persistent door in WORLD2_PERSISTENT -> GATE_OUT, in WORLD
const ROUNDS_REF: u32 = 0x3F0; // BULLETS without XCNT: the AMMO's Clip Rounds (10)
const GUARD: u32 = 0x181; // NPC_ carrying PISTOL, RIFLE, HELMET, HAT, SUIT, MINE
const LONG_GUN: u32 = 0x182; // WEAP with a model and an animation type (drawable), damage 10
const PLAYER_GUN: u32 = 0x183; // drawable WEAP, damage 30, DNAM flags 2 Player Only (the Van Graff rifles)
const THROWN_SPEAR: u32 = 0x184; // drawable WEAP, damage 40, Thrown (1 Hand): never held while a hand weapon is carried
const COMPANION_GUN: u32 = 0x17E; // WEAP, DNAM flags 1 Can't Drop | Non-Playable (Boone's rifle)
const COMPANION_AMMO: u32 = 0x17F; // AMMO, DATA flags Non-Playable (AmmoCompanion)

fn plugin_bytes() -> Vec<u8> {
    let misc = cat(&[
        &record(b"MISC", 0, STIMPAK, &[sub(b"EDID", z("TestStim")), sub(b"DATA", value_weight(25, 0.5))]),
        &record(b"MISC", 0, CAPS, &[sub(b"EDID", z("TestCaps")), sub(b"DATA", value_weight(1, 0.0))]),
    ]);
    let weightless = [
        top(b"NOTE", &record(b"NOTE", 0, NOTE, &[sub(b"EDID", z("TestNote"))])),
        top(b"CHIP", &record(b"CHIP", 0, CHIP, &[sub(b"EDID", z("TestChip"))])),
        top(b"CCRD", &record(b"CCRD", 0, CARD, &[sub(b"EDID", z("TestCard")), sub(b"DATA", 2u32.to_le_bytes().to_vec())])),
        top(b"CMNY", &record(b"CMNY", 0, CMNY, &[sub(b"EDID", z("TestMoney")), sub(b"DATA", 40u32.to_le_bytes().to_vec())])),
    ]
    .concat();
    let keym = cat(&[
        &record(b"KEYM", 0, KEY, &[sub(b"EDID", z("TestKey")), sub(b"DATA", value_weight(0, 0.0))]),
        &record(b"KEYM", 0x400, QUEST_KEY, &[sub(b"EDID", z("TestQuestKey")), sub(b"DATA", value_weight(0, 0.0))]),
    ]);
    let furn = record(b"FURN", 0, CHAIR, &[sub(b"EDID", z("TestChair"))]);
    let lvli = cat(&[
        &record(
            b"LVLI",
            0,
            LL_LOOT,
            &[sub(b"EDID", z("TestLoot")), sub(b"LVLD", vec![0]), sub(b"LVLF", vec![0]), lvlo(1, STIMPAK, 2), lvlo(5, CAPS, 10)],
        ),
        &record(b"LVLI", 0, LL_NONE, &[sub(b"EDID", z("TestNothing")), sub(b"LVLD", vec![100]), lvlo(1, STIMPAK, 1)]),
        &record(b"LVLI", 0, LL_LOOP, &[sub(b"EDID", z("TestLoop")), sub(b"LVLD", vec![0]), lvlo(1, LL_LOOP, 1)]),
        &record(
            b"LVLI",
            0,
            LL_ODDS,
            &[sub(b"EDID", z("TestOddsList")), sub(b"LVLD", vec![0]), sub(b"LVLG", ODDS.to_le_bytes().to_vec()), lvlo(1, STIMPAK, 1)],
        ),
        &record(
            b"LVLI",
            0,
            LL_WORN,
            &[
                sub(b"EDID", z("TestWornList")),
                sub(b"LVLD", vec![0]),
                sub(b"LVLF", vec![4]),
                lvlo(1, PISTOL, 1),
                sub(b"COED", cat(&[&0u32.to_le_bytes(), &0u32.to_le_bytes(), &0.4f32.to_le_bytes()])),
                lvlo(1, STIMPAK, 1),
            ],
        ),
    ]);
    let cont = record(
        b"CONT",
        0,
        SAFE,
        &[
            sub(b"EDID", z("TestSafe")),
            sub(b"FULL", z("Safe")),
            cnto(CAPS, 3),
            cnto(LL_LOOT, 1),
            cnto(STIMPAK, 0),
            sub(b"DATA", cat(&[&[2u8], &10f32.to_le_bytes()])),
        ],
    );
    let door = record(b"DOOR", 0, DOOR, &[sub(b"EDID", z("TestDoor")), sub(b"FNAM", vec![0])]);
    // S.P.E.C.I.A.L. 5,6,7,4,3,2,1 after base health.
    let npc_data = cat(&[&100i32.to_le_bytes(), &[5u8, 6, 7, 4, 3, 2, 1]]);
    let npc = record(b"NPC_", 0, 0x7, &[sub(b"EDID", z("Player")), cnto(CAPS, 5), cnto(GLOVE, 1), sub(b"DATA", npc_data)]);
    let mut dnam = vec![0u8; 120]; // required part of WEAP DNAM; projectile at +36
    dnam[36..40].copy_from_slice(&MINE_PROJ.to_le_bytes());
    let mine = {
        let mut m = weap(MINE, "TestMine", 25, 0.5);
        let extra = write_subrecords(&[sub(b"DNAM", dnam)]);
        let size = u32::from_le_bytes(m[4..8].try_into().unwrap()) + extra.len() as u32;
        m[4..8].copy_from_slice(&size.to_le_bytes());
        m.extend(extra);
        m
    };
    let long_gun = {
        let mut m = weap(LONG_GUN, "TestLongGun", 300, 8.0);
        let mut dnam = vec![0u8; 120];
        dnam[0..4].copy_from_slice(&5u32.to_le_bytes()); // Rifle - Ballistic (2 Hand)
        let extra = write_subrecords(&[sub(b"MODL", z("weapons/2handrifle/test.nif")), sub(b"DNAM", dnam)]);
        let size = u32::from_le_bytes(m[4..8].try_into().unwrap()) + extra.len() as u32;
        m[4..8].copy_from_slice(&size.to_le_bytes());
        m.extend(extra);
        m
    };
    let player_gun = {
        let data = cat(&[&300i32.to_le_bytes(), &100i32.to_le_bytes(), &8f32.to_le_bytes(), &30i16.to_le_bytes(), &[12u8]]);
        let mut dnam = vec![0u8; 120];
        dnam[0..4].copy_from_slice(&7u32.to_le_bytes()); // Rifle - Energy (2 Hand)
        dnam[56..60].copy_from_slice(&1u32.to_le_bytes()); // flags 2: Player Only (bit 0)
        record(b"WEAP", 0, PLAYER_GUN, &[sub(b"EDID", z("TestPlayerGun")), sub(b"DATA", data), sub(b"MODL", z("weapons/2handrifle/test2.nif")), sub(b"DNAM", dnam)])
    };
    let thrown_spear = {
        let data = cat(&[&50i32.to_le_bytes(), &100i32.to_le_bytes(), &1f32.to_le_bytes(), &40i16.to_le_bytes(), &[1u8]]);
        let mut dnam = vec![0u8; 120];
        dnam[0..4].copy_from_slice(&13u32.to_le_bytes()); // Thrown (1 Hand)
        record(b"WEAP", 0, THROWN_SPEAR, &[sub(b"EDID", z("TestThrowingSpear")), sub(b"DATA", data), sub(b"MODL", z("weapons/thrown/spear.nif")), sub(b"DNAM", dnam)])
    };
    let companion_gun = {
        let mut m = weap(COMPANION_GUN, "TestCompanionGun", 0, 8.0);
        let mut dnam = vec![0u8; 120];
        dnam[12] = 0x88; // flags 1: Can't Drop (bit 3), Non-Playable (bit 7)
        let extra = write_subrecords(&[sub(b"DNAM", dnam)]);
        let size = u32::from_le_bytes(m[4..8].try_into().unwrap()) + extra.len() as u32;
        m[4..8].copy_from_slice(&size.to_le_bytes());
        m.extend(extra);
        m
    };
    let weaps = cat(&[&weap(PISTOL, "TestPistol", 100, 3.0), &weap(RIFLE, "TestRifle", 300, 8.0), &mine, &long_gun, &player_gun, &thrown_spear, &companion_gun]);
    let guard = record(
        b"NPC_",
        0,
        GUARD,
        &[
            sub(b"EDID", z("TestGuard")),
            cnto(PISTOL, 1),
            cnto(RIFLE, 1),
            cnto(HELMET, 1),
            cnto(HAT, 1),
            cnto(SUIT, 1),
            cnto(MINE, 2),
            cnto(LONG_GUN, 1),
            cnto(PLAYER_GUN, 1),
            cnto(THROWN_SPEAR, 3),
            sub(b"DATA", cat(&[&50i32.to_le_bytes(), &[5u8, 5, 5, 5, 5, 5, 5]])),
        ],
    );
    let proj = record(b"PROJ", 0, MINE_PROJ, &[sub(b"EDID", z("TestMineProjectile"))]);
    let armos = cat(&[&armo(HELMET, "TestHelmet", 0b11, 2.0), &armo(HAT, "TestHat", 0b10, 1.0), &armo(SUIT, "TestSuit", 0b100, 15.0), &armo_flags(GLOVE, "TestGlove", 0b1000, 0x40, 0.0)]);
    let enit = cat(&[&20i32.to_le_bytes(), &[0u8, 0, 0, 0], &0u32.to_le_bytes(), &0f32.to_le_bytes(), &0u32.to_le_bytes()]);
    let alch = record(b"ALCH", 0, AID, &[sub(b"EDID", z("TestAid")), sub(b"DATA", 0.5f32.to_le_bytes().to_vec()), sub(b"ENIT", enit)]);
    let ammo_data = cat(&[&1f32.to_le_bytes(), &[0u8, 0, 0, 0], &2i32.to_le_bytes(), &[10u8]]);
    let ammo_dat2 = cat(&[&1u32.to_le_bytes(), &0u32.to_le_bytes(), &0.1f32.to_le_bytes()]);
    let companion_ammo_data = cat(&[&1f32.to_le_bytes(), &[2u8, 0, 0, 0], &0i32.to_le_bytes(), &[1u8]]); // flags: Non-Playable
    let ammo = cat(&[
        &record(b"AMMO", 0, BULLETS, &[sub(b"EDID", z("TestBullets")), sub(b"DATA", ammo_data), sub(b"DAT2", ammo_dat2.clone())]),
        &record(b"AMMO", 0, COMPANION_AMMO, &[sub(b"EDID", z("TestCompanionAmmo")), sub(b"DATA", companion_ammo_data), sub(b"DAT2", ammo_dat2)]),
    ]);
    let coed = cat(&[&0u32.to_le_bytes(), &0u32.to_le_bytes(), &0.75f32.to_le_bytes()]);
    let locker = record(
        b"CONT",
        0,
        LOCKER,
        &[sub(b"EDID", z("TestLocker")), cnto(PISTOL, 1), sub(b"COED", coed), sub(b"DATA", cat(&[&[0u8], &1f32.to_le_bytes()]))],
    );
    let cell_a = record(b"CELL", 0, CELL_A, &[sub(b"EDID", z("TestCellA"))]);
    let cell_b = record(b"CELL", 0, CELL_B, &[sub(b"EDID", z("TestCellB"))]);
    let temp_a = cat(&[
        &refr(SAFE_REF, 0, SAFE, [1.0, 2.0, 3.0], vec![sub(b"EDID", z("TestSafeRef")), xloc(50, KEY)]),
        &refr(STIM_REF, 0, STIMPAK, [4.0, 5.0, 6.0], vec![sub(b"XCNT", 3i32.to_le_bytes().to_vec())]),
        &refr(DOOR_A, 0, DOOR, [0.0; 3], vec![xtel(DOOR_B, [100.0, 0.0, 0.0])]),
        &refr(PLAIN_DOOR, 0, DOOR, [0.0; 3], vec![]),
        &refr(CHAIR_REF, 0, CHAIR, [0.0; 3], vec![sub(b"XMRC", SAFE_REF.to_le_bytes().to_vec())]),
        &refr(CHILD_SAME, 0, STIMPAK, [0.0; 3], vec![xesp(KEY_REF, false)]),
        &refr(CHILD_OPPOSITE, 0, STIMPAK, [0.0; 3], vec![xesp(KEY_REF, true)]),
        &refr(GRANDCHILD, 0, STIMPAK, [0.0; 3], vec![xesp(CHILD_OPPOSITE, false)]),
        &refr(GUN_REF, 0, PISTOL, [0.0; 3], vec![sub(b"XHLP", 0.25f32.to_le_bytes().to_vec())]),
        &refr(ROUNDS_REF, 0, BULLETS, [0.0; 3], vec![]),
        &{
            // A placed explosive: same subrecords as REFR, record type PGRE.
            let mut v = refr(MINE_REF, 0, MINE_PROJ, [0.0; 3], vec![]);
            v[..4].copy_from_slice(b"PGRE");
            v
        },
    ]);
    let pers_a = refr(KEY_REF, 0x400 | 0x800, KEY, [7.0, 8.0, 9.0], vec![sub(b"EDID", z("TestKeyRef"))]);
    let temp_b = cat(&[
        &refr(DOOR_B, 0, DOOR, [0.0; 3], vec![xtel(DOOR_A, [-100.0, 0.0, 0.0])]),
        &refr(IN_DOOR, 0, DOOR, [0.0; 3], vec![xtel(EXT_DOOR, [-73162.9, 1312.4, 8686.4])]),
    ]);
    let wrld = {
        let wrec = record(b"WRLD", 0, WORLD, &[sub(b"EDID", z("TestWorld"))]);
        let pcell = record(b"CELL", 0, WORLD_PERSISTENT, &[sub(b"DATA", vec![0])]);
        let pdoor = cat(&[
            &refr(EXT_DOOR, 0x400, DOOR, [-73100.0, 1300.0, 8680.0], vec![xtel(IN_DOOR, [0.0; 3])]),
            &refr(GATE_OUT, 0x400, DOOR, [-73300.0, 1500.0, 8690.0], vec![xtel(GATE_IN, [8288.0, 6896.0, 1008.0])]),
        ]);
        let xclc = cat(&[&(-18i32).to_le_bytes(), &0i32.to_le_bytes()]);
        let ecell = record(b"CELL", 0, WORLD_CELL, &[sub(b"EDID", z("TestGrid")), sub(b"DATA", vec![0]), sub(b"XCLC", xclc)]);
        let block = group(0i32.to_le_bytes(), 4, &group(0i32.to_le_bytes(), 5, &cat(&[&ecell, &cell_children(WORLD_CELL, &[], &[])])));
        let children = cat(&[&pcell, &cell_children(WORLD_PERSISTENT, &[], &pdoor), &block]);
        let world = cat(&[&wrec, &group(WORLD.to_le_bytes(), 1, &children)]);
        // A second worldspace, a child of the first that uses only its map data, like
        // FreesideNorthWorld (PNAM 4) [observed in FalloutNV.esm].
        let wrec2 = record(
            b"WRLD",
            0,
            WORLD2,
            &[sub(b"EDID", z("TestWorld2")), sub(b"WNAM", WORLD.to_le_bytes().to_vec()), sub(b"PNAM", 4u16.to_le_bytes().to_vec())],
        );
        let pcell2 = record(b"CELL", 0, WORLD2_PERSISTENT, &[sub(b"DATA", vec![0])]);
        let gate_in = refr(GATE_IN, 0x400, DOOR, [8600.0, 7000.0, 1024.0], vec![xtel(GATE_OUT, [-73250.0, 1450.0, 8690.0])]);
        let xclc2 = cat(&[&2i32.to_le_bytes(), &1i32.to_le_bytes()]);
        let ecell2 = record(b"CELL", 0, WORLD2_CELL, &[sub(b"EDID", z("TestGrid2")), sub(b"DATA", vec![0]), sub(b"XCLC", xclc2)]);
        let block2 = group(0i32.to_le_bytes(), 4, &group(0i32.to_le_bytes(), 5, &cat(&[&ecell2, &cell_children(WORLD2_CELL, &[], &[])])));
        let children2 = cat(&[&pcell2, &cell_children(WORLD2_PERSISTENT, &[], &gate_in), &block2]);
        let world2 = cat(&[&wrec2, &group(WORLD2.to_le_bytes(), 1, &children2)]);
        top(b"WRLD", &cat(&[&world, &world2]))
    };
    let cells = top(
        b"CELL",
        &group(
            0i32.to_le_bytes(),
            2,
            &group(
                0i32.to_le_bytes(),
                3,
                &cat(&[&cell_a, &cell_children(CELL_A, &temp_a, &pers_a), &cell_b, &cell_children(CELL_B, &temp_b, &[])]),
            ),
        ),
    );
    let glob = cat(&[
        &glob_rec(0x35, "GameYear", b's', 2281.0),
        &glob_rec(0x36, "GameMonth", b's', 9.0),
        &glob_rec(0x37, "GameDay", b's', 31.0),
        &glob_rec(0x38, "GameHour", b's', 12.0),
        &glob_rec(0x39, "GameDaysPassed", b'f', 5.0),
        &glob_rec(0x3A, "TimeScale", b's', 30.0),
        &glob_rec(0x150, "TestGlobal", b'f', 42.0),
        &glob_rec(ODDS, "TestOdds", b's', 100.0),
    ]);
    let gmst = cat(&[
        &record(b"GMST", 0, 0x160, &[sub(b"EDID", z("fAVDCarryWeightsBase")), sub(b"DATA", 150f32.to_le_bytes().to_vec())]),
        &record(b"GMST", 0, 0x161, &[sub(b"EDID", z("sOverEncumbered")), sub(b"DATA", z("Too heavy"))]),
        &record(b"GMST", 0, 0x162, &[sub(b"EDID", z("fAVDCarryWeightMult")), sub(b"DATA", 10f32.to_le_bytes().to_vec())]),
    ]);
    cat(&[
        &tes4(),
        &top(b"GLOB", &glob),
        &top(b"GMST", &gmst),
        &top(b"MISC", &misc),
        &top(b"KEYM", &keym),
        &weightless,
        &top(b"FURN", &furn),
        &top(b"LVLI", &lvli),
        &top(b"CONT", &cat(&[&cont, &locker])),
        &top(b"WEAP", &weaps),
        &top(b"ARMO", &armos),
        &top(b"ALCH", &alch),
        &top(b"AMMO", &ammo),
        &top(b"PROJ", &proj),
        &top(b"DOOR", &door),
        &top(b"NPC_", &cat(&[&npc, &guard])),
        &cells,
        &wrld,
    ])
}

fn schema() -> Schema {
    Schema::load_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../../schemas/fnv")).unwrap()
}

struct Fixture {
    forms: FormStore,
    refs: RefTable,
}

fn fixture() -> Fixture {
    let plugins = vec![Plugin::parse(&plugin_bytes(), "Test.esm".into()).unwrap()];
    let lo = LoadOrder::new(&plugins).unwrap();
    let s = schema();
    Fixture { forms: FormStore::build(&lo, &s), refs: RefTable::build(&lo, &s) }
}

fn id(x: u32) -> FormId {
    FormId(x)
}

// ---- loaders --------------------------------------------------------------------

#[test]
fn forms_load_through_the_schema() {
    let f = fixture();
    assert!(f.forms.issues.is_empty(), "{:?}", f.forms.issues.to_json());
    let stim = f.forms.item(id(STIMPAK)).unwrap();
    assert_eq!((stim.kind, stim.value, stim.weight), (ItemKind::Misc, Some(25), Some(0.5)));
    assert_eq!(f.forms.by_edid("teststim"), Some(id(STIMPAK)));
    assert_eq!(f.forms.globals[&id(0x150)], 42.0);
    let BaseData::Container(c) = &f.forms.get(id(SAFE)).unwrap().data else { panic!() };
    assert!(c.respawns);
    assert_eq!(
        c.entries,
        vec![Entry::new(id(CAPS), 3), Entry::new(id(LL_LOOT), 1), Entry::new(id(STIMPAK), 0)]
    );
    let l = f.forms.leveled(id(LL_LOOT)).unwrap();
    assert_eq!(l.entries.len(), 2);
    assert_eq!((l.entries[1].level, l.entries[1].item, l.entries[1].count), (5, id(CAPS), 10));
    assert_eq!(f.forms.leveled(id(LL_NONE)).unwrap().chance_none, 100);
    assert!(matches!(f.forms.get(id(0x7)).unwrap().data, BaseData::Actor(_)));
    assert_eq!(f.forms.setting_f32("FAVDCARRYWEIGHTSBASE"), Some(150.0));
    assert_eq!(f.forms.settings["soverencumbered"], fnv_sim::settings::Setting::Str("Too heavy".into()));
}

#[test]
fn references_load_with_cells_and_extra_data() {
    let f = fixture();
    assert!(f.refs.issues.is_empty(), "{:?}", f.refs.issues.to_json());
    assert_eq!(f.refs.refs.len(), 17);
    // Stored in the persistent cell, parented to the grid cell containing it.
    assert_eq!(f.refs.get(id(EXT_DOOR)).unwrap().stored_in, Some(id(WORLD_PERSISTENT)));
    assert_eq!(f.refs.get(id(EXT_DOOR)).unwrap().cell, Some(id(WORLD_CELL)));
    assert!(f.refs.by_cell[&id(WORLD_CELL)].contains(&id(EXT_DOOR)));
    assert_eq!(f.refs.exterior_cell_at(id(WORLD_PERSISTENT), [-73162.9, 1312.4, 0.0]), Some(id(WORLD_CELL)));
    assert_eq!(f.refs.exterior_cell_at(id(WORLD_PERSISTENT), [0.0, 0.0, 0.0]), None);
    assert_eq!(f.refs.exterior_cell_at(id(CELL_A), [-73162.9, 1312.4, 0.0]), None);
    assert!(f.refs.issues.is_empty(), "{:?}", f.refs.issues.to_json());
    assert_eq!(f.refs.cell_by_edid("testcella"), Some(id(CELL_A)));
    assert!(f.refs.by_cell[&id(CELL_A)].contains(&id(SAFE_REF)));
    let safe = f.refs.get(id(SAFE_REF)).unwrap();
    assert_eq!((safe.base, safe.cell, safe.pos), (id(SAFE), Some(id(CELL_A)), [1.0, 2.0, 3.0]));
    assert_eq!(safe.lock.unwrap().key, Some(id(KEY)));
    assert_eq!(safe.lock.unwrap().level, 50);
    assert_eq!(f.refs.get(id(STIM_REF)).unwrap().count, Some(3));
    let a = f.refs.get(id(DOOR_A)).unwrap().teleport.unwrap();
    assert_eq!((a.door, a.pos, a.rot), (id(DOOR_B), [100.0, 0.0, 0.0], [0.0, 0.0, 1.5]));
    assert_eq!(f.refs.get(id(DOOR_B)).unwrap().cell, Some(id(CELL_B)));
    let key = f.refs.get(id(KEY_REF)).unwrap();
    assert!(key.in_persistent_group && key.initially_disabled());
    assert_eq!(f.refs.by_edid("TESTSAFEREF"), Some(id(SAFE_REF)));
    // XMRC (decoded through the common layout) marks the merchant's stock container.
    assert_eq!(f.refs.get(id(CHAIR_REF)).unwrap().merchant_container, Some(id(SAFE_REF)));
    assert!(f.refs.is_merchant_container(id(SAFE_REF)));
    assert!(!f.refs.is_merchant_container(id(CHAIR_REF)));
}

// ---- runtime --------------------------------------------------------------------

#[test]
fn locked_container_needs_its_key() {
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig { seed: 1, ..SimConfig::default() });
    let p = FormId::PLAYER_REF;
    assert_eq!(w.item_count(p, id(CAPS)).unwrap(), 5, "player starts with the base form's items");
    let r = w.activate(p, id(SAFE_REF)).unwrap();
    assert_eq!(r.outcome, Outcome::Locked { level: 50 });
    w.add_item(p, id(KEY), 1).unwrap();
    let r = w.activate(p, id(SAFE_REF)).unwrap();
    assert_eq!(r.outcome, Outcome::OpenedContainer { container: id(SAFE_REF) });
    assert_eq!(r.unlocked_with, Some(id(KEY)));
    // Player level 1: the leveled list can only give the level-1 entry (2 stimpaks);
    // the count-0 entry adds nothing.
    assert_eq!(w.item_count(id(SAFE_REF), id(CAPS)).unwrap(), 3);
    assert_eq!(w.item_count(id(SAFE_REF), id(STIMPAK)).unwrap(), 2);
    let moved = w.transfer_all(id(SAFE_REF), p).unwrap();
    assert_eq!(moved, vec![(id(STIMPAK), 2), (id(CAPS), 3)]);
    assert_eq!(w.item_count(p, id(CAPS)).unwrap(), 8);
    assert!(w.peek(id(SAFE_REF)).unwrap().inventory.as_ref().unwrap().is_empty());
}

/// An AddItem on a placed container nothing has read yet (no state, cell never loaded,
/// never opened) joins its authored contents: GetItemCount, a save and load, leaving and
/// entering its cell before the respawn delay and taking everything all keep or move it
/// (#3021: Old World Blues' terminal puts the X-13 Document in the X-13 test safe this
/// way; what took it out in that run was the test's own failure script).
#[test]
fn add_item_before_a_container_is_read_is_kept() {
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig { seed: 1, ..SimConfig::default() });
    let p = FormId::PLAYER_REF;
    assert!(w.peek(id(SAFE_REF)).is_none(), "the safe has no state yet");
    w.add_item(id(SAFE_REF), id(QUEST_KEY), 1).unwrap();
    assert_eq!(w.item_count(id(SAFE_REF), id(QUEST_KEY)).unwrap(), 1);
    assert_eq!(w.item_count(id(SAFE_REF), id(CAPS)).unwrap(), 3, "the authored contents are there too");
    let mut w = World::restore(&f.forms, &f.refs, &w.snapshot()).unwrap();
    w.detach_cell(id(CELL_A)).unwrap();
    w.advance_hours(1.0).unwrap();
    assert!(w.attach_cell(id(CELL_A)).unwrap().is_empty(), "no reset before the respawn delay");
    assert_eq!(w.item_count(id(SAFE_REF), id(QUEST_KEY)).unwrap(), 1);
    w.unlock(id(SAFE_REF)).unwrap();
    assert_eq!(w.activate(p, id(SAFE_REF)).unwrap().outcome, Outcome::OpenedContainer { container: id(SAFE_REF) });
    let moved = w.transfer_all(id(SAFE_REF), p).unwrap();
    assert!(moved.contains(&(id(QUEST_KEY), 1)), "{moved:?}");
    assert_eq!(w.item_count(p, id(QUEST_KEY)).unwrap(), 1);
}

#[test]
fn picking_up_and_dropping_items() {
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let p = FormId::PLAYER_REF;
    let r = w.activate(p, id(STIM_REF)).unwrap();
    assert_eq!(r.outcome, Outcome::PickedUp { item: id(STIMPAK), count: 3, owner: None });
    assert_eq!(w.item_count(p, id(STIMPAK)).unwrap(), 3);
    // Activating the taken reference again gives its stack again (observed in game).
    assert_eq!(w.activate(p, id(STIM_REF)).unwrap().outcome, Outcome::PickedUp { item: id(STIMPAK), count: 3, owner: None });
    assert_eq!(w.item_count(p, id(STIMPAK)).unwrap(), 6);
    w.remove_item(p, id(STIMPAK), 3).unwrap();
    let dropped = w.drop_item(p, id(STIMPAK), 2).unwrap()[0];
    assert_eq!(dropped, FormId::FIRST_DYNAMIC);
    assert_eq!(w.item_count(p, id(STIMPAK)).unwrap(), 1);
    assert_eq!(w.activate(p, dropped).unwrap().outcome, Outcome::PickedUp { item: id(STIMPAK), count: 2, owner: None });
    assert_eq!(w.item_count(p, id(STIMPAK)).unwrap(), 3);
    // Scripted activation of a disabled item still gives it and leaves it disabled
    // (observed in game).
    assert!(matches!(w.activate(p, id(KEY_REF)).unwrap().outcome, Outcome::PickedUp { .. }));
    assert!(!w.is_enabled(id(KEY_REF)).unwrap());
    assert_eq!(w.item_count(p, id(KEY)).unwrap(), 1);
    // A disabled door does nothing (hypothesis).
    w.set_enabled(id(PLAIN_DOOR), false).unwrap();
    assert_eq!(w.activate(p, id(PLAIN_DOOR)).unwrap().outcome, Outcome::Disabled);
}

#[test]
fn doors_teleport_or_toggle() {
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let p = FormId::PLAYER_REF;
    let r = w.activate(p, id(DOOR_A)).unwrap();
    assert_eq!(
        r.outcome,
        Outcome::Teleported { to_door: id(DOOR_B), cell: Some(id(CELL_B)), pos: [100.0, 0.0, 0.0], rot: [0.0, 0.0, 1.5] }
    );
    let ps = w.peek(p).unwrap();
    assert_eq!((ps.cell, ps.pos), (Some(id(CELL_B)), [100.0, 0.0, 0.0]));
    w.activate(p, id(DOOR_B)).unwrap();
    assert_eq!(w.peek(p).unwrap().cell, Some(id(CELL_A)));
    assert_eq!(w.activate(p, id(PLAIN_DOOR)).unwrap().outcome, Outcome::DoorToggled { open: true });
    assert_eq!(w.activate(p, id(PLAIN_DOOR)).unwrap().outcome, Outcome::DoorToggled { open: false });
    w.lock(id(PLAIN_DOOR), Some(25)).unwrap();
    assert_eq!(w.activate(p, id(PLAIN_DOOR)).unwrap().outcome, Outcome::Locked { level: 25 });
    w.unlock(id(PLAIN_DOOR)).unwrap();
    assert!(matches!(w.activate(p, id(PLAIN_DOOR)).unwrap().outcome, Outcome::DoorToggled { .. }));
    assert_eq!(w.activate(p, id(CHAIR_REF)).unwrap().outcome, Outcome::Activated);
}

/// A stand-in for the physics placement `fnvtool sim run` supplies.
struct Lift;

impl actions::Settle for Lift {
    fn settle(&mut self, _: &World, cell: Option<FormId>, pos: [f32; 3]) -> Option<([f32; 3], serde_json::Value)> {
        (cell == Some(id(CELL_B))).then(|| ([pos[0], pos[1], pos[2] + 3.5], serde_json::json!({"shell": 3.5})))
    }
}

#[test]
fn teleported_actors_are_settled_by_the_caller() {
    let f = fixture();
    let text = format!(
        r#"{{"format": "fnv.sim.actions/1", "actions": [
            {{"op": "activate", "actor": "player", "target": "0x{DOOR_A:08X}"}},
            {{"op": "get_pos", "of": "player", "label": "after A"}},
            {{"op": "activate", "actor": "player", "target": "0x{DOOR_B:08X}"}},
            {{"op": "get_pos", "of": "player", "label": "after B"}}]}}"#
    );
    let script: Script = serde_json::from_str(&text).unwrap();
    let mut w = World::new(&f.forms, &f.refs, actions::config(&script));
    let t = actions::run_with(&mut w, &script, Some(&mut Lift)).unwrap();
    assert_eq!(t["steps"][0]["result"]["pos"], serde_json::json!([100.0, 0.0, 0.0]));
    assert_eq!(t["steps"][0]["result"]["settled"]["pos"], serde_json::json!([100.0, 0.0, 3.5]));
    assert_eq!(t["observations"][0]["pos"], serde_json::json!([100.0, 0.0, 3.5]));
    // No settling offered (another cell): the stored destination stands.
    assert!(t["steps"][2]["result"].get("settled").is_none());
    assert_eq!(t["observations"][1]["pos"], serde_json::json!([-100.0, 0.0, 0.0]));
    // Without a settler, `run` keeps the stored destination.
    let mut w = World::new(&f.forms, &f.refs, actions::config(&script));
    let t = actions::run(&mut w, &script).unwrap();
    assert_eq!(t["observations"][0]["pos"], serde_json::json!([100.0, 0.0, 0.0]));
}

#[test]
fn errors_are_reported_not_panics() {
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let p = FormId::PLAYER_REF;
    assert_eq!(w.activate(p, id(0xDEAD)), Err(SimError::UnknownRef(id(0xDEAD))));
    assert_eq!(w.add_item(p, id(DOOR), 1), Err(SimError::NotAnItem(id(DOOR))));
    assert_eq!(w.add_item(p, id(0xBEEF), 1), Err(SimError::UnknownForm(id(0xBEEF))));
    assert_eq!(w.add_item(p, id(CAPS), 0), Err(SimError::BadCount(0)));
    assert_eq!(w.add_item(id(DOOR_A), id(CAPS), 1), Err(SimError::NoInventory(id(DOOR_A))));
    assert!(matches!(w.add_item(p, id(LL_LOOP), 1), Err(SimError::Leveled(_))));
    assert_eq!(w.remove_item(p, id(STIMPAK), 4).unwrap(), 0);
}

// ---- leveled lists --------------------------------------------------------------

#[test]
fn leveled_lists_follow_level_and_chance_none() {
    let f = fixture();
    let mut rng = ChaCha8Rng::seed_from_u64(7);
    for _ in 0..200 {
        let mut out = Vec::new();
        leveled::resolve(&f.forms, id(LL_NONE), 1, 99, &mut rng, &mut out).unwrap();
        assert!(out.is_empty(), "chance none 100 never yields");
        let mut out = Vec::new();
        leveled::resolve(&f.forms, id(LL_LOOT), 2, 10, &mut rng, &mut out).unwrap();
        // Without "all levels" only the highest eligible level (5) is chosen; one roll
        // multiplied by the requested count.
        assert_eq!(out, vec![Entry::new(id(CAPS), 20)]);
    }
}

/// AddItemToLeveledList: an entry added at run time takes part in the choice as an
/// authored one, is kept in snapshots, and only leveled item lists take entries.
/// EquipItem's NoUnequipFlag and UnequipItem's NoEquipFlag as the world keeps them: a
/// locked-on item cannot be taken off nor its slot taken; a locked-off one cannot be put
/// on; taking the lock away (what the script functions do first) frees it; snapshots keep
/// it [documented: GECK wiki EquipItem, UnequipItem].
#[test]
fn script_locks_hold_items_on_and_off() {
    use fnv_sim::world::EquipLock;
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let p = FormId::PLAYER_REF;
    for it in [PISTOL, RIFLE, HAT, HELMET] {
        w.add_item(p, id(it), 1).unwrap();
    }
    w.equip(p, id(PISTOL)).unwrap();
    w.lock_equip(p, id(PISTOL), Some(EquipLock::NoUnequip)).unwrap();
    assert_eq!(w.unequip(p, id(PISTOL)), Err(SimError::EquipLocked { item: id(PISTOL), on: true }));
    assert_eq!(w.equip(p, id(RIFLE)).map(|e| e.unequipped), Err(SimError::EquipLocked { item: id(PISTOL), on: true }), "the slot is held");
    // Other slots are free.
    assert!(w.equip(p, id(HAT)).is_ok());
    let snap: fnv_sim::Snapshot = serde_json::from_str(&serde_json::to_string(&w.snapshot()).unwrap()).unwrap();
    let mut back = World::restore(&f.forms, &f.refs, &snap).unwrap();
    assert_eq!(back.equip_lock(p, id(PISTOL)), Some(EquipLock::NoUnequip));
    w.lock_equip(p, id(PISTOL), None).unwrap();
    assert!(w.unequip(p, id(PISTOL)).unwrap());
    // Locked off: not put on until the lock goes.
    w.lock_equip(p, id(HELMET), Some(EquipLock::NoEquip)).unwrap();
    assert_eq!(w.equip(p, id(HELMET)).map(|e| e.consumed), Err(SimError::EquipLocked { item: id(HELMET), on: false }));
    w.lock_equip(p, id(HELMET), None).unwrap();
    assert_eq!(w.equip(p, id(HELMET)).unwrap().unequipped, vec![id(HAT)]);
    assert!(back.unequip(p, id(HAT)).is_ok(), "a lock on one item leaves the others");
}

/// RemoveAllItems' retain-ownership flag: moved items keep their owner only when asked
/// [documented: GECK wiki RemoveAllItems].
#[test]
fn moving_everything_keeps_the_owner_only_when_asked() {
    use fnv_sim::inventory::Extra;
    let f = fixture();
    let p = FormId::PLAYER_REF;
    let owned = Extra { owner: Some(id(GUARD)), ..Extra::default() };
    for keep in [true, false] {
        let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
        w.get(id(SAFE_REF)).unwrap().inventory.get_or_insert_with(Default::default).add_extra(id(STIMPAK), 2, owned);
        w.transfer_all_owned(id(SAFE_REF), p, keep).unwrap();
        let inv = w.peek(p).unwrap().inventory.clone().unwrap();
        // (The safe's leveled entry may give plain stimpaks too.)
        let owned = inv.groups().any(|g| g.0 == id(STIMPAK) && g.1.owner == Some(id(GUARD)));
        assert_eq!(owned, keep, "keep {keep}");
        assert!(inv.count(id(STIMPAK)) >= 2);
    }
}

/// AddItemToLeveledList's health: the items the entry gives have that condition, also
/// through a nested list [documented: GECK wiki AddItemToLeveledList].
#[test]
fn a_scripts_leveled_entry_gives_items_at_its_condition() {
    use fnv_sim::forms::LeveledEntry;
    let f = fixture();
    let cfg = SimConfig { seed: 3, player_level: 10, ..SimConfig::default() };
    let mut w = World::new(&f.forms, &f.refs, cfg);
    let p = FormId::PLAYER_REF;
    w.add_to_leveled_list(id(LL_LOOT), LeveledEntry { level: 7, item: id(PISTOL), count: 1, condition: Some(0.6) }).unwrap();
    assert_eq!(w.add_item(p, id(LL_LOOT), 1).unwrap(), vec![(id(PISTOL), 1)]);
    let inv = w.peek(p).unwrap().inventory.clone().unwrap();
    assert_eq!(inv.groups().find(|g| g.0 == id(PISTOL)).unwrap().1.condition, Some(0.6));
}

#[test]
fn a_leveled_entrys_coed_gives_items_at_its_condition() {
    // The CondNV… lists of the masters: a COED after an LVLO is that entry's condition.
    let f = fixture();
    let l = f.forms.leveled(id(LL_WORN)).unwrap();
    assert_eq!(l.entries.iter().map(|e| e.condition).collect::<Vec<_>>(), vec![Some(0.4), None]);
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let p = FormId::PLAYER_REF;
    w.add_item(p, id(LL_WORN), 1).unwrap();
    let inv = w.peek(p).unwrap().inventory.clone().unwrap();
    assert_eq!(inv.groups().find(|g| g.0 == id(PISTOL)).unwrap().1.condition, Some(0.4));
    assert_eq!(inv.groups().find(|g| g.0 == id(STIMPAK)).unwrap().1.condition, None);
}

/// `World::set_player_level`: what AddItem, PlaceAtMe and a reference with no fixed level
/// roll at from then on. A snapshot keeps the configured level, not this one: the game sets
/// it again. LL_LOOT: level 1 stimpaks, level 5 caps.
#[test]
fn the_players_level_moves_the_rolls_until_a_restore() {
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let p = FormId::PLAYER_REF;
    assert_eq!(w.add_item(p, id(LL_LOOT), 1).unwrap(), vec![(id(STIMPAK), 2)]);
    w.set_player_level(5);
    assert_eq!((w.roll_level(), w.config().player_level), (5, 1));
    assert_eq!(w.add_item(p, id(LL_LOOT), 1).unwrap(), vec![(id(CAPS), 10)]);
    // SAFE holds 3 caps and LL_LOOT: created and placed alike, the list gives the caps.
    let made = w.place(id(SAFE), None, [0.0; 3], [0.0; 3], 1).unwrap();
    for safe in [made, id(SAFE_REF)] {
        assert_eq!(w.item_count(safe, id(CAPS)).unwrap(), 13);
        assert_eq!(w.item_count(safe, id(STIMPAK)).unwrap(), 0);
    }
    let snap: fnv_sim::Snapshot = serde_json::from_str(&serde_json::to_string(&w.snapshot()).unwrap()).unwrap();
    let mut back = World::restore(&f.forms, &f.refs, &snap).unwrap();
    assert_eq!(back.roll_level(), 1);
    assert_eq!(back.add_item(p, id(LL_LOOT), 1).unwrap(), vec![(id(STIMPAK), 2)]);
    // Never below 1.
    back.set_player_level(0);
    assert_eq!(back.roll_level(), 1);
}

const ROOM: u32 = 0x410; // no zone
const ZONED_ROOM: u32 = 0x411; // in LOCK_ZONE
const LOCK_ZONE: u32 = 0x420; // ECZN, minimum level 11
const LEVELED_DOOR: u32 = 0x400; // lock 25, leveled, in ROOM
const ZONED_DOOR: u32 = 0x401; // lock 25, leveled, in ZONED_ROOM
const ZONED_CHEST: u32 = 0x402; // SAFE (LL_LOOT, Respawns), merchant stock, in ZONED_ROOM
const SHOPKEEPER_REF: u32 = 0x403; // XMRC -> ZONED_CHEST
const LEVELED_CHEST: u32 = 0x404; // SAFE, lock 25, leveled, in ROOM

/// Leveled locks and a merchant chest, with and without an encounter zone.
fn levels_fixture() -> Fixture {
    let xloc_leveled = || sub(b"XLOC", cat(&[&[25, 0, 0, 0], &0u32.to_le_bytes(), &[4, 0, 0, 0]]));
    let cell = |fid: u32, edid: &str, zone: Option<u32>, refs: &[u8]| {
        let mut subs = vec![sub(b"EDID", z(edid)), sub(b"DATA", vec![1])];
        subs.extend(zone.map(|z| sub(b"XEZN", z.to_le_bytes().to_vec())));
        cat(&[&record(b"CELL", 0, fid, &subs), &group(fid.to_le_bytes(), 6, &group(fid.to_le_bytes(), 9, refs))])
    };
    let zoned = cat(&[
        &refr(ZONED_DOOR, 0, DOOR, [0.0; 3], vec![xloc_leveled()]),
        &refr(ZONED_CHEST, 0, SAFE, [0.0; 3], vec![]),
        &refr(SHOPKEEPER_REF, 0, CHAIR, [0.0; 3], vec![sub(b"XMRC", ZONED_CHEST.to_le_bytes().to_vec())]),
    ]);
    let plain = cat(&[&refr(LEVELED_DOOR, 0, DOOR, [0.0; 3], vec![xloc_leveled()]), &refr(LEVELED_CHEST, 0, SAFE, [0.0; 3], vec![xloc_leveled()])]);
    let cells = cat(&[&cell(ROOM, "Room", None, &plain), &cell(ZONED_ROOM, "ZonedRoom", Some(LOCK_ZONE), &zoned)]);
    let glob = cat(&[
        &glob_rec(0x35, "GameYear", b's', 2281.0),
        &glob_rec(0x36, "GameMonth", b's', 9.0),
        &glob_rec(0x37, "GameDay", b's', 31.0),
        &glob_rec(0x38, "GameHour", b's', 12.0),
        &glob_rec(0x39, "GameDaysPassed", b'f', 5.0),
        &glob_rec(0x3A, "TimeScale", b's', 30.0),
    ]);
    let misc = cat(&[
        &record(b"MISC", 0, STIMPAK, &[sub(b"EDID", z("TestStim")), sub(b"DATA", value_weight(25, 0.5))]),
        &record(b"MISC", 0, CAPS, &[sub(b"EDID", z("TestCaps")), sub(b"DATA", value_weight(1, 0.0))]),
    ]);
    let bytes = cat(&[
        &tes4(),
        &top(b"GLOB", &glob),
        &top(b"MISC", &misc),
        &top(b"FURN", &record(b"FURN", 0, CHAIR, &[sub(b"EDID", z("TestChair"))])),
        &top(b"LVLI", &record(b"LVLI", 0, LL_LOOT, &[sub(b"EDID", z("TestLoot")), sub(b"LVLD", vec![0]), sub(b"LVLF", vec![0]), lvlo(1, STIMPAK, 2), lvlo(5, CAPS, 10)])),
        &top(b"CONT", &record(b"CONT", 0, SAFE, &[sub(b"EDID", z("TestSafe")), cnto(LL_LOOT, 1), sub(b"DATA", cat(&[&[2u8], &10f32.to_le_bytes()]))])),
        &top(b"DOOR", &record(b"DOOR", 0, DOOR, &[sub(b"EDID", z("TestDoor")), sub(b"FNAM", vec![0])])),
        &top(b"ECZN", &record(b"ECZN", 0, LOCK_ZONE, &[sub(b"EDID", z("LockZone")), sub(b"DATA", cat(&[&0u32.to_le_bytes(), &[0, 11, 0, 0]]))])),
        &top(b"CELL", &group(0i32.to_le_bytes(), 2, &group(0i32.to_le_bytes(), 3, &cells))),
    ]);
    let plugins = vec![Plugin::parse(&bytes, "Levels.esm".into()).unwrap()];
    let lo = LoadOrder::new(&plugins).unwrap();
    let s = schema();
    Fixture { forms: FormStore::build(&lo, &s), refs: RefTable::build(&lo, &s) }
}

/// A door gets no fixed level, so its lock keeps its authored level whatever the player's
/// level, with or without a zone, before and after its state is made.
#[test]
fn a_door_keeps_its_authored_lock_level_when_the_players_level_changes() {
    let f = levels_fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    for door in [id(LEVELED_DOOR), id(ZONED_DOOR)] {
        w.set_player_level(1);
        assert_eq!(w.lock_of(door).unwrap().level, 25);
        w.set_player_level(21);
        assert_eq!(w.lock_of(door).unwrap().level, 25);
    }
    assert_eq!(w.enter_zone(id(LOCK_ZONE), 21), Some(21));
    w.set_player_level(41);
    for door in [id(LEVELED_DOOR), id(ZONED_DOOR)] {
        assert_eq!(w.lock_of(door).unwrap().level, 25);
        w.get(door).unwrap();
        assert_eq!(w.lock_of(door).unwrap().level, 25);
    }
    // The configured level still counts, as a test or the runner sets it.
    let mut w = World::new(&f.forms, &f.refs, SimConfig { player_level: 11, ..SimConfig::default() });
    assert_eq!(w.lock_of(id(LEVELED_DOOR)).unwrap().level, 50);
    w.get(id(LEVELED_DOOR)).unwrap();
    assert_eq!(w.lock_of(id(LEVELED_DOOR)).unwrap().level, 50);
}

/// A container's leveled lock hardens from the level fixed at its cell's load, and only
/// from that: the player's level alone moves nothing, and a state made before the load
/// keeps the authored level.
#[test]
fn a_containers_lock_hardens_from_its_fixed_level_only() {
    let f = levels_fixture();
    let chest = id(LEVELED_CHEST);
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    w.set_player_level(21);
    assert_eq!(w.lock_of(chest).unwrap().level, 25);
    assert!(w.fix_level(chest, 21).is_some());
    assert_eq!(w.lock_of(chest).unwrap().level, 75);
    w.get(chest).unwrap();
    w.set_player_level(41);
    assert_eq!(w.lock_of(chest).unwrap().level, 75);
    // Read before its cell loads.
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    w.set_player_level(21);
    w.get(chest).unwrap();
    assert!(w.fix_level(chest, 21).is_some());
    assert_eq!(w.lock_of(chest).unwrap().level, 25);
}

/// A chest in a zone, read before its cell loads, rolls at the zone's level, not a level-1
/// player's: the list's caps (level 5), not its stimpaks. A restock reads the same level.
#[test]
fn a_chest_in_a_zone_rolls_at_the_zones_level_before_its_cell_loads_and_on_a_restock() {
    let f = levels_fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let chest = id(ZONED_CHEST);
    assert_eq!((w.item_count(chest, id(CAPS)).unwrap(), w.item_count(chest, id(STIMPAK)).unwrap()), (10, 0));
    // The fixture's clock starts at GameDaysPassed 5.0, noon: midnight 7 restocks.
    w.advance_hours(48.0).unwrap();
    assert_eq!(w.peek(chest).unwrap().last_restock, Some(7.0));
    assert_eq!((w.item_count(chest, id(CAPS)).unwrap(), w.item_count(chest, id(STIMPAK)).unwrap()), (10, 0));
}

#[test]
fn entries_added_to_a_leveled_list_count_and_persist() {
    use fnv_sim::forms::LeveledEntry;
    let f = fixture();
    let cfg = SimConfig { seed: 3, player_level: 10, ..SimConfig::default() };
    let mut w = World::new(&f.forms, &f.refs, cfg);
    // LL_LOOT picks among its highest eligible level (5: CAPS x10); an entry at level 7
    // becomes the only one of the highest level.
    w.add_to_leveled_list(id(LL_LOOT), LeveledEntry { level: 7, item: id(KEY), count: 2, condition: None }).unwrap();
    assert_eq!(w.add_item(FormId::PLAYER_REF, id(LL_LOOT), 1).unwrap(), vec![(id(KEY), 2)]);
    let snap: fnv_sim::Snapshot = serde_json::from_str(&serde_json::to_string(&w.snapshot()).unwrap()).unwrap();
    let mut again = World::restore(&f.forms, &f.refs, &snap).unwrap();
    assert_eq!(again.leveled_added(id(LL_LOOT)).len(), 1);
    assert_eq!(again.add_item(FormId::PLAYER_REF, id(LL_LOOT), 1).unwrap(), vec![(id(KEY), 2)]);
    assert!(w.add_to_leveled_list(id(STIMPAK), LeveledEntry { level: 1, item: id(KEY), count: 1, condition: None }).is_err(), "not a leveled list");
    assert!(w.add_to_leveled_list(id(LL_LOOT), LeveledEntry { level: 1, item: id(0xDEAD), count: 1, condition: None }).is_err(), "unknown form");
}

#[test]
fn lvlg_global_replaces_chance_none_at_its_current_value() {
    let f = fixture();
    let mut rng = ChaCha8Rng::seed_from_u64(3);
    let set = |v: f32| std::collections::BTreeMap::from([(id(ODDS), v)]);
    for _ in 0..100 {
        let mut out = Vec::new();
        // Authored value 100 overrides the list's own chance none 0.
        leveled::resolve(&f.forms, id(LL_ODDS), 1, 1, &mut rng, &mut out).unwrap();
        assert!(out.is_empty());
        leveled::resolve_in(&f.forms, &set(0.0), id(LL_ODDS), 1, 1, &mut rng, &mut out).unwrap();
        assert_eq!(out, vec![Entry::new(id(STIMPAK), 1)]);
    }
    let mut n = 0;
    for _ in 0..4000 {
        let mut out = Vec::new();
        leveled::resolve_in(&f.forms, &set(25.0), id(LL_ODDS), 1, 1, &mut rng, &mut out).unwrap();
        n += out.len();
    }
    assert!((2850..3150).contains(&n), "{n}");
}

#[test]
fn world_add_item_reads_lvlg_globals_set_at_run_time() {
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let p = FormId::PLAYER_REF;
    assert!(w.add_item(p, id(LL_ODDS), 1).unwrap().is_empty());
    w.set_global(id(ODDS), 0.0).unwrap();
    assert_eq!(w.add_item(p, id(LL_ODDS), 1).unwrap(), vec![(id(STIMPAK), 1)]);
}

#[test]
fn below_is_uniform_enough() {
    let mut rng = ChaCha8Rng::seed_from_u64(0);
    let mut hist = [0u32; 7];
    for _ in 0..70_000 {
        hist[leveled::below(&mut rng, 7) as usize] += 1;
    }
    for h in hist {
        assert!((9_400..10_600).contains(&h), "{hist:?}");
    }
}

// ---- determinism, snapshots, action scripts --------------------------------------

const SCRIPT: &str = r#"{
  "format": "fnv.sim.actions/1", "seed": 3, "player_level": 1,
  "actions": [
    {"op": "activate", "actor": "player", "target": "TestSafeRef"},
    {"op": "add_item", "to": "player", "item": "TestKey", "count": 1},
    {"op": "activate", "actor": "player", "target": "TestSafeRef"},
    {"op": "transfer", "from": "TestSafeRef", "to": "player", "item": "TestCaps", "count": 2},
    {"op": "activate", "actor": "player", "target": "0x00000301"},
    {"op": "remove_item", "from": "player", "item": "NoSuchThing", "count": 1},
    {"op": "get_item_count", "of": "player", "item": "TestStim", "label": "stims"},
    {"op": "activate", "actor": "player", "target": "0x00000302"},
    {"op": "drop_item", "from": "player", "item": "TestStim", "count": 1},
    {"op": "get_pos", "of": "player"},
    {"op": "remove_all_items", "from": "TestSafeRef"},
    {"op": "get_item_count", "of": "TestSafeRef", "item": "TestStim", "label": "safe stims"}
  ],
  "report": ["player", "TestSafeRef"]
}"#;

fn run_script(f: &Fixture) -> serde_json::Value {
    let s: Script = serde_json::from_str(SCRIPT).unwrap();
    let mut w = World::new(&f.forms, &f.refs, actions::config(&s));
    actions::run(&mut w, &s).unwrap()
}

#[test]
fn action_script_trace() {
    let f = fixture();
    let t = run_script(&f);
    assert_eq!(t["format"], "fnv.sim.trace/1");
    let steps = t["steps"].as_array().unwrap();
    assert_eq!(steps[0]["result"]["result"], "locked");
    assert_eq!(steps[2]["result"]["unlocked_with"], "0x00000102");
    assert_eq!(steps[3]["result"]["moved"], 2);
    assert_eq!(steps[4]["result"]["result"], "picked_up");
    assert!(steps[5]["error"].as_str().unwrap().contains("NoSuchThing"));
    // The script's `activate` is the scripted Activate: one item from the XCNT 3 stack.
    assert_eq!(steps[4]["result"]["count"], 1);
    assert_eq!(steps[6]["result"]["count"], 1);
    assert_eq!(steps[7]["result"]["cell"], "0x00000201");
    assert_eq!(steps[8]["result"]["references"], serde_json::json!(["0xFF000000"]));
    let obs = t["observations"].as_array().unwrap();
    assert_eq!(obs.len(), 3);
    assert_eq!((obs[0]["label"].as_str(), obs[0]["count"].as_i64()), (Some("stims"), Some(1)));
    assert_eq!((obs[1]["label"].as_str(), obs[1]["cell"].as_str()), (Some("step9"), Some("0x00000201")));
    assert_eq!(obs[1]["pos"], serde_json::json!([100.0, 0.0, 0.0]));
    assert_eq!(obs[2]["count"], 0);
    assert_eq!(steps[10]["result"]["removed"], serde_json::json!([["0x00000100", 2], ["0x00000101", 1]]));
    let player = &t["final"]["0x00000014"];
    assert_eq!(player["inventory"]["0x00000101"], 7);
    assert_eq!(player["inventory"]["0x00000102"], 1);
    assert!(player["inventory"].get("0x00000100").is_none(), "the one stimpak was dropped");
    assert_eq!(t["final"]["0x00000300"]["lock"]["locked"], false);
    // Same input, same output.
    assert_eq!(run_script(&f), t);
}

#[test]
fn snapshot_resumes_identically() {
    let f = fixture();
    let cfg = SimConfig { seed: 99, player_level: 5, ..SimConfig::default() };
    let ops = |w: &mut World| {
        let p = FormId::PLAYER_REF;
        for _ in 0..20 {
            w.add_item(p, id(LL_LOOT), 1).unwrap();
        }
    };
    let mut full = World::new(&f.forms, &f.refs, cfg);
    ops(&mut full);
    ops(&mut full);

    let mut first = World::new(&f.forms, &f.refs, cfg);
    ops(&mut first);
    let text = serde_json::to_string(&first.snapshot()).unwrap();
    let snap = serde_json::from_str(&text).unwrap();
    let mut resumed = World::restore(&f.forms, &f.refs, &snap).unwrap();
    ops(&mut resumed);
    assert_eq!(serde_json::to_value(resumed.snapshot()).unwrap(), serde_json::to_value(full.snapshot()).unwrap());

    let mut bad = snap.clone();
    bad.format = "other".into();
    assert!(World::restore(&f.forms, &f.refs, &bad).is_err());
}

#[test]
fn enable_parent_chains() {
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let p = FormId::PLAYER_REF;
    // KEY_REF starts disabled: the same-state child is disabled, the opposite child
    // enabled, and the grandchild follows the opposite child.
    assert!(!w.is_enabled(id(CHILD_SAME)).unwrap());
    assert!(w.is_enabled(id(CHILD_OPPOSITE)).unwrap());
    assert!(w.is_enabled(id(GRANDCHILD)).unwrap());
    // Item activation ignores the enabled state (observed in game), the state stays.
    assert!(matches!(w.activate(p, id(CHILD_SAME)).unwrap().outcome, Outcome::PickedUp { .. }));
    assert!(!w.is_enabled(id(CHILD_SAME)).unwrap());
    w.set_enabled(id(KEY_REF), true).unwrap();
    assert!(w.is_enabled(id(CHILD_SAME)).unwrap());
    assert!(!w.is_enabled(id(CHILD_OPPOSITE)).unwrap());
    assert!(!w.is_enabled(id(GRANDCHILD)).unwrap());
    // A child's own flag does not matter while it has a parent.
    w.set_enabled(id(CHILD_SAME), false).unwrap();
    assert!(w.is_enabled(id(CHILD_SAME)).unwrap());
    // Queries do not materialise references.
    let fresh = World::new(&f.forms, &f.refs, SimConfig::default());
    fresh.is_enabled(id(GRANDCHILD)).unwrap();
    assert_eq!(fresh.changed().count(), 0);
}

/// `World::presence_serial` moves with every change that can change whether a reference
/// is present (its `deleted` flag, `is_enabled` with its enable parents) and stands
/// still for reads and for writes that change nothing: the AI keys its obstacle poll
/// on it (`fnv_ai::Ai::obstacles_in_line`).
#[test]
fn presence_serial_moves_with_every_presence_change() {
    let mut f = fixture();
    f.refs.merchant_containers.clear();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let p = FormId::PLAYER_REF;
    let mut last = w.presence_serial();
    fn moved(w: &World, last: &mut u64, what: &str) {
        let now = w.presence_serial();
        assert_ne!(now, *last, "{what} did not move the presence serial");
        *last = now;
    }
    // Reads leave it.
    w.is_enabled(id(GRANDCHILD)).unwrap();
    assert_eq!(w.presence_serial(), last);
    // A placed reference's state starts with the flags presence read from its placement
    // (KEY_REF is Initially Disabled): made, it leaves the serial.
    assert!(!w.is_enabled(id(KEY_REF)).unwrap());
    w.get(id(KEY_REF)).unwrap();
    assert_eq!(w.presence_serial(), last, "materialising a state as placed");
    w.get(id(KEY_REF)).unwrap();
    assert_eq!(w.presence_serial(), last, "an existing state");
    // The player is not placed: its state makes it present.
    assert!(w.peek(p).is_none() && w.is_enabled(p).is_err());
    w.get(p).unwrap();
    moved(&w, &mut last, "materialising the player");
    // Enabling a parent changes its children: the serial says so.
    w.set_enabled(id(KEY_REF), true).unwrap();
    moved(&w, &mut last, "set_enabled");
    w.set_enabled(id(KEY_REF), true).unwrap();
    assert_eq!(w.presence_serial(), last, "the same flag again");
    w.set_deleted(id(KEY_REF), true).unwrap();
    moved(&w, &mut last, "set_deleted");
    w.set_deleted(id(KEY_REF), false).unwrap();
    moved(&w, &mut last, "set_deleted back");
    // An item picked up by activation.
    w.get(id(CHILD_SAME)).unwrap();
    assert_eq!(w.presence_serial(), last, "materialising the child as placed");
    assert!(matches!(w.activate(p, id(CHILD_SAME)).unwrap().outcome, Outcome::PickedUp { .. }));
    moved(&w, &mut last, "a pick-up");
    // Created references: placed and dropped.
    let made = w.place(id(SAFE), Some(id(CELL_A)), [0.0; 3], [0.0; 3], 1).unwrap();
    moved(&w, &mut last, "place");
    w.add_item(made, id(PISTOL), 1).unwrap();
    last = w.presence_serial();
    w.drop_item(made, id(PISTOL), 1).unwrap();
    moved(&w, &mut last, "a drop");
    // A respawning container reset (its state taken out).
    w.unlock(id(SAFE_REF)).unwrap();
    last = w.presence_serial();
    w.detach_cell(id(CELL_A)).unwrap();
    w.advance_hours(72.0).unwrap();
    assert_eq!(w.attach_cell(id(CELL_A)).unwrap(), vec![id(SAFE_REF)]);
    moved(&w, &mut last, "a cell reset");
    // Another world, a restored one too, never shares the number.
    let other = World::new(&f.forms, &f.refs, SimConfig::default());
    assert_ne!(other.presence_serial(), w.presence_serial());
    let restored = World::restore(&f.forms, &f.refs, &w.snapshot()).unwrap();
    assert_ne!(restored.presence_serial(), w.presence_serial());
}

/// Making a placed reference's state (`World::get`) does not move the presence serial,
/// as presence reads the same flags from the placement before; a change of a flag on
/// that state does: on a reference placed enabled, one placed Initially Disabled and
/// one under an enable parent.
#[test]
fn presence_serial_stands_still_for_a_state_made_as_placed() {
    let mut f = fixture();
    f.refs.merchant_containers.clear();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    for r in [id(SAFE_REF), id(KEY_REF), id(CHILD_SAME)] {
        let before = w.presence_serial();
        let was = w.is_enabled(r).unwrap();
        let own = w.get(r).unwrap().enabled();
        assert_eq!(w.presence_serial(), before, "{r:?} made as placed");
        assert_eq!(w.is_enabled(r).unwrap(), was, "{r:?} reads as before");
        w.set_enabled(r, !own).unwrap();
        assert_ne!(w.presence_serial(), before, "{r:?} switched");
        let before = w.presence_serial();
        w.set_deleted(r, true).unwrap();
        assert_ne!(w.presence_serial(), before, "{r:?} deleted");
    }
}

#[test]
fn clock_lives_in_the_time_globals() {
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let c = w.advance_hours(13.0).unwrap();
    assert_eq!((c.month, c.day, c.hour), (10.0, 1.0, 1.0));
    assert_eq!(w.global(id(0x38)).unwrap(), 1.0);
    // 120 real seconds at TimeScale 30 = 1 game hour.
    let c = w.advance_real_seconds(120.0).unwrap();
    assert_eq!(c.hour, 2.0);
    w.set_global(id(0x150), 7.0).unwrap();
    assert_eq!(w.global(id(0x150)).unwrap(), 7.0);
    assert!(w.set_global(id(STIMPAK), 1.0).is_err());
    // Changed globals are part of the snapshot.
    let snap = w.snapshot();
    assert_eq!(snap.globals[&id(0x150)], 7.0);
    let back = World::restore(&f.forms, &f.refs, &snap).unwrap();
    assert_eq!(back.calendar().unwrap(), w.calendar().unwrap());
}

const GAME_YEAR: u32 = 0x35;
const GAME_MONTH: u32 = 0x36;
const GAME_DAY: u32 = 0x37;
const GAME_HOUR: u32 = 0x38;
const GAME_DAYS_PASSED: u32 = 0x39;
const TIME_SCALE: u32 = 0x3A;

/// Frames of `dt` real seconds, as the running game calls the clock; the game days they took.
fn clock_frames(w: &mut World, dt: f64, frames: u64) -> f64 {
    let scale = w.global(id(TIME_SCALE)).unwrap() as f64;
    for _ in 0..frames {
        w.advance_real_seconds(dt).unwrap();
    }
    frames as f64 * dt * scale / 3600.0 / 24.0
}

fn close_to(value: f32, want: f64, tolerance: f64) -> bool {
    (value as f64 - want).abs() <= tolerance
}

/// Two f32 steps of GameDaysPassed just past day 1000 (one is about 6.1e-5 days).
const PAST_DAY_1000: f64 = 1.3e-4;

#[test]
fn clock_globals_follow_frames_past_day_1000() {
    let f = fixture();
    for (scale, fps) in [(1.0, 60.0), (1.0, 240.0), (30.0, 60.0), (30.0, 240.0)] {
        let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
        w.set_global(id(TIME_SCALE), scale as f32).unwrap();
        w.set_global(id(GAME_DAYS_PASSED), 1000.0).unwrap();
        w.set_global(id(GAME_HOUR), 0.0).unwrap();
        // Ten game minutes, 7e-3 days: far more than the tolerance, had it stopped.
        let frames = (600.0 / scale * fps) as u64;
        let elapsed = clock_frames(&mut w, 1.0 / fps, frames);
        let what = format!("TimeScale {scale} at {fps} frames a second");
        assert!(close_to(w.global(id(GAME_DAYS_PASSED)).unwrap(), 1000.0 + elapsed, PAST_DAY_1000), "{what}");
        assert!(close_to(w.global(id(GAME_HOUR)).unwrap(), elapsed * 24.0, 1e-3), "{what}");
    }
}

#[test]
fn script_writes_to_the_time_globals_win_over_the_running_clock() {
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    clock_frames(&mut w, 1.0 / 60.0, 600);
    let (hour, days) = (w.global(id(GAME_HOUR)).unwrap(), w.global(id(GAME_DAYS_PASSED)).unwrap());

    // Set GameDaysPassed: frames go on from there, the hour does not jump.
    w.set_global(id(GAME_DAYS_PASSED), 2000.0).unwrap();
    assert_eq!(w.calendar().unwrap().days_passed, 2000.0);
    let elapsed = clock_frames(&mut w, 1.0 / 60.0, 600);
    assert!(close_to(w.global(id(GAME_DAYS_PASSED)).unwrap(), 2000.0 + elapsed, 2.5e-4));
    assert!(close_to(w.global(id(GAME_HOUR)).unwrap(), hour as f64 + elapsed * 24.0, 1e-3));

    // Set GameHour: the same, and GameDaysPassed does not jump.
    let days_now = w.global(id(GAME_DAYS_PASSED)).unwrap();
    w.set_global(id(GAME_HOUR), 3.5).unwrap();
    let elapsed = clock_frames(&mut w, 1.0 / 60.0, 600);
    assert!(close_to(w.global(id(GAME_HOUR)).unwrap(), 3.5 + elapsed * 24.0, 1e-3));
    assert!(close_to(w.global(id(GAME_DAYS_PASSED)).unwrap(), days_now as f64 + elapsed, 2.5e-4));

    // Backwards as well.
    w.set_global(id(GAME_DAYS_PASSED), days).unwrap();
    w.advance_hours(1.0).unwrap();
    assert!(close_to(w.global(id(GAME_DAYS_PASSED)).unwrap(), days as f64 + 1.0 / 24.0, 1e-5));

    // GameDay, GameMonth and GameYear live in the globals alone: the next wrap counts on
    // from what was set.
    w.set_global(id(GAME_YEAR), 2300.0).unwrap();
    w.set_global(id(GAME_MONTH), 1.0).unwrap();
    w.set_global(id(GAME_DAY), 28.0).unwrap();
    w.set_global(id(GAME_HOUR), 23.0).unwrap();
    let c = w.advance_hours(2.0).unwrap();
    assert_eq!((c.year, c.month, c.day, c.hour), (2300.0, 2.0, 1.0, 1.0));

    // TimeScale set: read every frame, and 0 holds the clock.
    w.set_global(id(TIME_SCALE), 0.0).unwrap();
    let held = w.calendar().unwrap();
    clock_frames(&mut w, 1.0 / 60.0, 60);
    assert_eq!(w.calendar().unwrap(), held);
    w.set_global(id(TIME_SCALE), 7200.0).unwrap();
    assert!(close_to(w.advance_real_seconds(0.5).unwrap().hour, 2.0, 1e-5));
}

#[test]
fn a_write_to_a_time_global_counts_even_when_the_value_is_the_same() {
    let f = fixture();
    // GameDaysPassed: four frames round back to 1000, and the write of 1000 is the time.
    let frame = 1.0 / 60.0;
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    w.set_global(id(GAME_DAYS_PASSED), 1000.0).unwrap();
    clock_frames(&mut w, frame, 4);
    assert_eq!(w.global(id(GAME_DAYS_PASSED)).unwrap(), 1000.0);
    w.set_global(id(GAME_DAYS_PASSED), 1000.0).unwrap();
    clock_frames(&mut w, frame, 2);
    assert_eq!(w.global(id(GAME_DAYS_PASSED)).unwrap(), 1000.0, "two frames after the write");
    clock_frames(&mut w, frame, 4);
    assert!(w.global(id(GAME_DAYS_PASSED)).unwrap() > 1000.0, "six frames after the write");

    // Written away and back before the next frame is the same.
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    w.set_global(id(GAME_DAYS_PASSED), 1000.0).unwrap();
    clock_frames(&mut w, frame, 4);
    w.set_global(id(GAME_DAYS_PASSED), 5.0).unwrap();
    w.set_global(id(GAME_DAYS_PASSED), 1000.0).unwrap();
    clock_frames(&mut w, frame, 2);
    assert_eq!(w.global(id(GAME_DAYS_PASSED)).unwrap(), 1000.0);

    // GameHour, on its own: a step under half its f32 step, and GameDaysPassed goes on.
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    w.set_global(id(TIME_SCALE), 1.0).unwrap();
    w.set_global(id(GAME_HOUR), 16.0).unwrap();
    let dt = 1.0 / 1000.0;
    clock_frames(&mut w, dt, 3);
    let carried = w.snapshot().clock_days_passed;
    assert_eq!(w.global(id(GAME_HOUR)).unwrap(), 16.0);
    w.set_global(id(GAME_HOUR), 16.0).unwrap();
    assert_eq!(w.snapshot().clock_hour, None);
    assert_eq!(w.snapshot().clock_days_passed, carried);
    clock_frames(&mut w, dt, 3);
    assert_eq!(w.global(id(GAME_HOUR)).unwrap(), 16.0, "the three frames before the write are gone");

    // The clock's own writes go on, frame after frame.
    let elapsed = clock_frames(&mut w, dt, 2000);
    assert!(close_to(w.global(id(GAME_HOUR)).unwrap(), 16.0 + (elapsed + 3.0 * dt / 86400.0) * 24.0, 1e-5));
}

#[test]
fn advancing_to_an_hour_reaches_midnight_and_the_date() {
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    w.set_global(id(GAME_DAY), 18.0).unwrap();
    // Start 12, one frame at TimeScale 30 and 60 frames a second.
    clock_frames(&mut w, 1.0 / 60.0, 1);
    let hours = w.advance_to_hour(0.0).unwrap();
    assert!((hours - 12.0).abs() < 1e-3, "{hours}");
    let c = w.calendar().unwrap();
    assert_eq!((c.day, c.hour), (19.0, 0.0));
    // The hour it shows is no move, and leaves nothing behind.
    assert_eq!(w.advance_to_hour(0.0).unwrap(), 0.0);
    assert_eq!(w.calendar().unwrap(), c);
}

/// Through JSON and back, as a save goes.
fn reload<'a>(f: &'a Fixture, w: &World) -> World<'a> {
    let text = serde_json::to_string(&w.snapshot()).unwrap();
    World::restore(&f.forms, &f.refs, &serde_json::from_str(&text).unwrap()).unwrap()
}

#[test]
fn save_and_load_keep_the_clock() {
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    w.set_global(id(GAME_DAYS_PASSED), 1000.0).unwrap();
    clock_frames(&mut w, 1.0 / 60.0, 3600);
    let mut r = reload(&f, &w);
    assert_eq!(r.calendar().unwrap(), w.calendar().unwrap());
    assert_eq!(r.snapshot(), w.snapshot());
    // A world loaded with the clock far on goes on from it as the running one does.
    clock_frames(&mut r, 1.0 / 60.0, 3600);
    clock_frames(&mut w, 1.0 / 60.0, 3600);
    assert_eq!(r.snapshot(), w.snapshot());
}

#[test]
fn a_restored_run_matches_the_uninterrupted_one_through_a_restock() {
    let f = fixture();
    let (p, safe) = (FormId::PLAYER_REF, id(SAFE_REF));
    let frame = 1.0 / 60.0;
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    w.unlock(safe).unwrap();
    w.transfer_all(safe, p).unwrap();
    // Day 1001 is a restock day (7 x 143); a game day is 172800 frames here.
    w.set_global(id(GAME_DAYS_PASSED), 1000.0).unwrap();
    clock_frames(&mut w, frame, 172_793);
    let mut r = reload(&f, &w);
    for at in 172_794..=172_830 {
        clock_frames(&mut w, frame, 1);
        clock_frames(&mut r, frame, 1);
        assert_eq!(r.snapshot(), w.snapshot(), "frame {at}");
        let restock = (at >= 172_795).then_some(1001.0);
        assert_eq!(r.peek(safe).unwrap().last_restock, restock, "frame {at}");
    }
    assert_eq!(w.global(id(GAME_DAYS_PASSED)).unwrap(), 1001.0002);
    assert_eq!(w.item_count(safe, id(CAPS)).unwrap(), 3);
}

#[test]
fn restoring_every_frame_does_not_stop_the_clock() {
    let f = fixture();
    let frame = 1.0 / 60.0;
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    w.set_global(id(GAME_DAYS_PASSED), 1000.0).unwrap();
    let mut r = reload(&f, &w);
    for _ in 0..2000 {
        clock_frames(&mut w, frame, 1);
        clock_frames(&mut r, frame, 1);
        r = reload(&f, &r);
    }
    assert_eq!(r.snapshot(), w.snapshot());
    assert!(r.global(id(GAME_DAYS_PASSED)).unwrap() > 1000.01);
}

#[test]
fn a_restore_just_before_midnight_rolls_the_date_on_the_same_frame() {
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    w.set_global(id(TIME_SCALE), 1.0).unwrap();
    w.set_global(id(GAME_HOUR), 23.0).unwrap();
    let c = w.advance_hours(1.0 - 1e-9).unwrap();
    assert!(c.hour < 24.0);
    let mut r = reload(&f, &w);
    assert_eq!(r.calendar().unwrap(), c);
    clock_frames(&mut w, 1.0 / 240.0, 1);
    clock_frames(&mut r, 1.0 / 240.0, 1);
    assert_eq!(r.calendar().unwrap(), w.calendar().unwrap());
    assert_ne!(r.calendar().unwrap().day, c.day);
    assert_eq!(r.snapshot(), w.snapshot());
}

#[test]
fn a_save_without_the_clock_copies_or_with_stale_ones_goes_on_from_the_globals() {
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    w.set_global(id(GAME_DAYS_PASSED), 1000.0).unwrap();
    clock_frames(&mut w, 1.0 / 60.0, 3600);
    let stored = w.calendar().unwrap();

    // An older save: the fields are absent, and it still loads.
    let mut json = serde_json::to_value(w.snapshot()).unwrap();
    let fields = json.as_object_mut().unwrap();
    assert!(fields.remove("clock_hour").is_some() && fields.remove("clock_days_passed").is_some());
    let mut old = World::restore(&f.forms, &f.refs, &serde_json::from_value(json).unwrap()).unwrap();
    assert_eq!(old.calendar().unwrap(), stored);
    let before = old.global(id(GAME_DAYS_PASSED)).unwrap() as f64;
    let elapsed = clock_frames(&mut old, 1.0 / 60.0, 3600);
    assert!(close_to(old.global(id(GAME_DAYS_PASSED)).unwrap(), before + elapsed, PAST_DAY_1000));

    // A global edited in the save: the copy that no longer rounds to it does not count.
    let mut snap = w.snapshot();
    snap.globals.insert(id(GAME_DAYS_PASSED), 7.0);
    let mut edited = World::restore(&f.forms, &f.refs, &snap).unwrap();
    assert_eq!(edited.calendar().unwrap().days_passed, 7.0);
    let c = edited.advance_hours(24.0).unwrap();
    assert!((c.days_passed - 8.0).abs() < 1e-5);
    assert_eq!(edited.snapshot().clock_hour, w.snapshot().clock_hour);
}

#[test]
fn merchants_restock_at_a_midnight_crossed_by_frames_past_day_1000() {
    // The stopped GameDaysPassed never reached the next midnight (#1377). Day 1001 is a
    // restock day (7 x 143).
    let f = fixture();
    let (p, safe) = (FormId::PLAYER_REF, id(SAFE_REF));
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    w.unlock(safe).unwrap();
    w.transfer_all(safe, p).unwrap();
    w.set_global(id(GAME_DAYS_PASSED), 1000.999).unwrap();
    // 0.0017 days at TimeScale 30 and 60 frames a second.
    clock_frames(&mut w, 1.0 / 60.0, 300);
    assert_eq!(w.peek(safe).unwrap().last_restock, Some(1001.0));
    assert_eq!(w.item_count(safe, id(CAPS)).unwrap(), 3);
}

#[test]
fn condition_and_ownership_travel_with_items() {
    use fnv_sim::inventory::Extra;
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let p = FormId::PLAYER_REF;
    // COED on the container entry.
    let BaseData::Container(c) = &f.forms.get(id(LOCKER)).unwrap().data else { panic!() };
    assert_eq!(c.entries[0].extra.condition, Some(0.75));
    // The locker is not placed; give the player its pistol through a placed gun (XHLP).
    assert_eq!(f.refs.get(id(GUN_REF)).unwrap().health, Some(0.25));
    w.activate(p, id(GUN_REF)).unwrap();
    w.add_item(p, id(PISTOL), 1).unwrap();
    let inv = w.peek(p).unwrap().inventory.clone().unwrap();
    let groups: Vec<_> = inv.groups().filter(|g| g.0 == id(PISTOL)).map(|g| (g.1, g.2)).collect();
    assert_eq!(groups, vec![(Extra::default(), 1), (Extra { condition: Some(0.25), ..Extra::default() }, 1)]);
    // Dropping both makes one reference per group, keeping the condition.
    let refs = w.drop_item(p, id(PISTOL), 2).unwrap();
    assert_eq!(refs.len(), 2);
    assert_eq!(w.peek(refs[1]).unwrap().extra.condition, Some(0.25));
    w.activate(p, refs[1]).unwrap();
    let inv = w.peek(p).unwrap().inventory.clone().unwrap();
    assert_eq!(inv.groups().find(|g| g.0 == id(PISTOL)).unwrap().1.condition, Some(0.25));
    // Snapshots keep extra data.
    let snap = serde_json::to_string(&w.snapshot()).unwrap();
    let back = World::restore(&f.forms, &f.refs, &serde_json::from_str(&snap).unwrap()).unwrap();
    assert_eq!(back.peek(p), w.peek(p));
}

/// The Equip Broken Items tweak's flag lets the player, and only the player, equip one.
#[test]
fn the_player_equips_a_broken_weapon_under_the_tweak() {
    use fnv_sim::inventory::Extra;
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let p = FormId::PLAYER_REF;
    w.add_item_extra(p, id(PISTOL), 1, Extra { condition: Some(0.0), ..Extra::default() }).unwrap();
    w.set_player_equips_broken(true);
    w.equip(p, id(PISTOL)).unwrap();
    assert!(w.peek(p).unwrap().inventory.as_ref().unwrap().is_equipped(id(PISTOL)));
}

#[test]
fn a_broken_weapon_is_not_equipped() {
    use fnv_sim::inventory::Extra;
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let p = FormId::PLAYER_REF;
    // At zero condition the weapon stays carried but cannot be equipped.
    w.add_item_extra(p, id(PISTOL), 1, Extra { condition: Some(0.0), ..Extra::default() }).unwrap();
    assert_eq!(w.equip(p, id(PISTOL)), Err(SimError::Broken(id(PISTOL))));
    assert_eq!(w.item_count(p, id(PISTOL)).unwrap(), 1);
    // A second, worn but working, one is equipped instead of the broken one.
    w.add_item_extra(p, id(PISTOL), 1, Extra { condition: Some(0.3), ..Extra::default() }).unwrap();
    w.equip(p, id(PISTOL)).unwrap();
    let inv = w.peek(p).unwrap().inventory.clone().unwrap();
    assert_eq!(inv.condition_in_use(id(PISTOL)), Some(0.3));
    // Dropping the equipped one leaves the broken one carried; the reference on the
    // ground keeps its condition and is not equipped.
    let dropped = w.drop_equipped(p, id(PISTOL)).unwrap();
    assert_eq!(dropped.len(), 1);
    assert_eq!(w.peek(dropped[0]).unwrap().extra, Extra { condition: Some(0.3), ..Extra::default() });
    assert_eq!(w.item_count(p, id(PISTOL)).unwrap(), 1);
    assert_eq!(w.equip(p, id(PISTOL)), Err(SimError::Broken(id(PISTOL))));
    assert!(w.drop_equipped(p, id(PISTOL)).unwrap().is_empty());
    // Broken armour is still worn (only weapons are refused).
    w.add_item_extra(p, id(SUIT), 1, Extra { condition: Some(0.0), ..Extra::default() }).unwrap();
    w.equip(p, id(SUIT)).unwrap();
}

/// A live unit (a planted explosive, `Extra::live`, #1640) stays with its holder: no
/// equip, removal, drop or transfer takes it, while the ordinary units of the same item
/// are used as usual; it is kept in snapshots.
#[test]
fn a_live_unit_is_not_equipped_removed_dropped_or_moved() {
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let p = FormId::PLAYER_REF;
    let mine = id(MINE);
    w.add_item(p, mine, 2).unwrap();
    assert!(w.inventory_mut(p).unwrap().mark_live(mine));
    // The ordinary unit is equipped, then removed; the live one is neither.
    w.equip(p, mine).unwrap();
    assert!(w.unequip(p, mine).unwrap());
    assert_eq!(w.remove_item(p, mine, 5).unwrap(), 1);
    assert_eq!(w.item_count(p, mine).unwrap(), 1);
    assert_eq!(w.equip(p, mine), Err(SimError::NotCarried { actor: p, item: mine }));
    assert!(w.drop_item(p, mine, 1).unwrap().is_empty());
    assert!(w.player_drop_item(mine, 1).unwrap().is_empty());
    assert_eq!(w.transfer(p, id(SAFE_REF), mine, 1).unwrap(), 0);
    assert!(!w.transfer_all(p, id(SAFE_REF)).unwrap().iter().any(|s| s.0 == mine));
    w.add_item(p, id(STIMPAK), 1).unwrap();
    let removed = w.remove_all_items(p).unwrap();
    assert!(removed.iter().any(|s| s.0 == id(STIMPAK)) && !removed.iter().any(|s| s.0 == mine), "{removed:?}");
    let inv = w.peek(p).unwrap().inventory.clone().unwrap();
    assert_eq!((inv.count(mine), inv.live_count(mine), inv.free_count(mine)), (1, 1, 0));
    // A second, ordinary, unit goes where any would.
    w.add_item(p, mine, 1).unwrap();
    assert_eq!(w.transfer(p, id(SAFE_REF), mine, 5).unwrap(), 1);
    assert_eq!(w.item_count(id(SAFE_REF), mine).unwrap(), 1);
    // Kept through a snapshot.
    let snap = serde_json::to_string(&w.snapshot()).unwrap();
    let back = World::restore(&f.forms, &f.refs, &serde_json::from_str(&snap).unwrap()).unwrap();
    assert_eq!(back.peek(p).unwrap().inventory.as_ref().unwrap().live_count(mine), 1);
}

/// A resurrected actor's own items are made up again (World::restock_inventory, #2683):
/// what was taken comes back, what it still has is not doubled, what it was given stays.
#[test]
fn a_restocked_inventory_gets_back_what_is_missing_of_its_own_list() {
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let g = w.place(id(GUARD), None, [0.0; 3], [0.0; 3], 1).unwrap();
    let before = w.peek(g).unwrap().inventory.clone().unwrap();
    // Nothing missing: nothing added.
    assert!(w.restock_inventory(g).unwrap().is_empty());
    // Its gun and one of its two mines are gone (dropped beside the body, looted); it was
    // given five caps.
    let inv = w.inventory_mut(g).unwrap();
    assert_eq!(inv.take(id(LONG_GUN), 1).len(), 1);
    assert_eq!(inv.take(id(MINE), 1).len(), 1);
    w.add_item(g, id(CAPS), 5).unwrap();
    let mut added = w.restock_inventory(g).unwrap();
    added.sort();
    let mut want = vec![(id(LONG_GUN), 1), (id(MINE), 1)];
    want.sort();
    assert_eq!(added, want);
    let inv = w.peek(g).unwrap().inventory.clone().unwrap();
    for (item, n) in before.iter() {
        assert_eq!(inv.count(item), n, "{item:?}");
    }
    assert_eq!(inv.count(id(CAPS)), 5, "what it was given stays");
    // It holds the gun again; the suit it never lost is still worn, once.
    assert!(inv.is_equipped(id(LONG_GUN)) && inv.is_equipped(id(SUIT)));
    assert_eq!(inv.equipped().len(), before.equipped().len());
}

/// A rebuilt inventory (ResetInventory, a resurrection's empty pick) keeps the live units
/// of planted explosives (#1640): their plants still find them.
#[test]
fn a_reset_inventory_keeps_live_units() {
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let g = w.place(id(GUARD), None, [0.0; 3], [0.0; 3], 1).unwrap();
    let mine = id(MINE);
    assert_eq!(w.item_count(g, mine).unwrap(), 2);
    assert!(w.inventory_mut(g).unwrap().mark_live(mine));
    w.add_item(g, id(CAPS), 5).unwrap();
    assert!(w.reset_inventory(g).unwrap());
    let inv = w.peek(g).unwrap().inventory.clone().unwrap();
    assert_eq!((inv.count(mine), inv.live_count(mine), inv.count(id(CAPS))), (3, 1, 0), "rolled again, the live one kept");
    w.clear_inventory_but_live(g).unwrap();
    let inv = w.peek(g).unwrap().inventory.clone().unwrap();
    assert_eq!((inv.len(), inv.count(mine), inv.live_count(mine)), (1, 1, 1));
    // Without a live unit it is no inventory at all, as before.
    assert!(w.inventory_mut(g).unwrap().take_live(mine));
    w.clear_inventory_but_live(g).unwrap();
    assert!(w.peek(g).unwrap().inventory.is_none());
}

/// A menu row whose units are no longer carried fails before anything is taken off
/// (#1083): the clashing armour, and the item itself, stay worn.
#[test]
fn a_failed_equip_leaves_the_worn_gear_on() {
    use fnv_sim::inventory::Pick;
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let p = FormId::PLAYER_REF;
    for it in [HAT, HELMET, SUIT, PISTOL, RIFLE] {
        w.add_item(p, id(it), 1).unwrap();
    }
    for it in [HAT, SUIT, PISTOL] {
        w.equip(p, id(it)).unwrap();
    }
    let worn = |w: &World| w.peek(p).unwrap().inventory.as_ref().unwrap().equipped();
    let before = worn(&w);
    let half = Pick::at(Some(0.5));
    let gone = |item| Err(SimError::NotCarried { actor: p, item: id(item) });
    // No helmet at half condition is carried: the hat it would replace stays on.
    assert_eq!(w.equip_at(p, id(HELMET), half).map(|e| e.unequipped), gone(HELMET));
    // The suit clashes with itself: the worn one stays on.
    assert_eq!(w.equip_at(p, id(SUIT), half).map(|e| e.unequipped), gone(SUIT));
    // The same for weapons: another weapon is refused by the sound-unit check (which
    // reports it as Broken), the equipped one by the new check.
    assert_eq!(w.equip_at(p, id(RIFLE), half).map(|e| e.unequipped), Err(SimError::Broken(id(RIFLE))));
    assert_eq!(w.equip_at(p, id(PISTOL), half).map(|e| e.unequipped), gone(PISTOL));
    assert_eq!(worn(&w), before);
    // A unit that is carried still replaces the clashing one.
    assert_eq!(w.equip_at(p, id(HELMET), Pick::ANY).unwrap().unequipped, vec![id(HAT)]);
}

/// Reading a stat or a lock of a reference nothing has touched rolls none of its
/// leveled lists, so later rolls don't depend on what was only looked at (#1083).
#[test]
fn reading_values_leaves_the_random_stream_alone() {
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let pos = |w: &World| w.snapshot().rng_word_pos;
    let start = pos(&w);
    // The safe holds a leveled list (LL_LOOT).
    assert_eq!(w.lock_status(id(SAFE_REF)).unwrap(), 1);
    assert_eq!(w.lock_status(id(CHAIR_REF)).unwrap(), 0);
    assert!(w.peek(id(SAFE_REF)).is_none(), "not materialised by a read");
    assert_eq!(w.actor_value(FormId::PLAYER_REF, "strength").unwrap(), 5.0);
    assert!(w.peek(FormId::PLAYER_REF).is_none());
    assert_eq!(pos(&w), start);
    assert!(matches!(w.lock_status(id(0xDEAD)), Err(SimError::UnknownRef(_))));
    // Once the safe is materialised, the same values come back.
    w.get(id(SAFE_REF)).unwrap();
    assert_eq!(w.lock_status(id(SAFE_REF)).unwrap(), 1);
}

#[test]
fn equipping_follows_slots() {
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let p = FormId::PLAYER_REF;
    for it in [PISTOL, RIFLE, HELMET, HAT, SUIT, AID, CAPS] {
        w.add_item(p, id(it), 1).unwrap();
    }
    assert!(w.equip(p, id(PISTOL)).unwrap().unequipped.is_empty());
    assert_eq!(w.equip(p, id(RIFLE)).unwrap().unequipped, vec![id(PISTOL)]);
    w.equip(p, id(HAT)).unwrap();
    w.equip(p, id(SUIT)).unwrap();
    // The helmet covers Head and Hair: it replaces the hat, not the suit.
    assert_eq!(w.equip(p, id(HELMET)).unwrap().unequipped, vec![id(HAT)]);
    let inv = w.peek(p).unwrap().inventory.clone().unwrap();
    // The player's glove (non-playable, worn from the start) stays on.
    assert_eq!(inv.equipped(), vec![id(RIFLE), id(HELMET), id(SUIT), id(GLOVE)]);
    // Aid items are consumed; other items cannot be equipped.
    assert!(w.equip(p, id(AID)).unwrap().consumed);
    assert_eq!(w.item_count(p, id(AID)).unwrap(), 0);
    assert_eq!(w.equip(p, id(CAPS)), Err(SimError::NotEquippable(id(CAPS))));
    assert_eq!(w.equip(p, id(AID)), Err(SimError::NotCarried { actor: p, item: id(AID) }));
    // Giving away an equipped item unequips it.
    w.transfer(p, id(SAFE_REF), id(RIFLE), 1).unwrap();
    assert!(!w.peek(id(SAFE_REF)).unwrap().inventory.as_ref().unwrap().is_equipped(id(RIFLE)));
    assert!(w.unequip(p, id(SUIT)).unwrap());
    assert!(!w.unequip(p, id(SUIT)).unwrap());
}

#[test]
fn encumbrance_uses_strength_and_settings() {
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let p = FormId::PLAYER_REF;
    assert_eq!(w.actor_value(p, "Strength").unwrap(), 5.0);
    assert_eq!(w.actor_value(p, "luck").unwrap(), 1.0);
    assert!(matches!(w.actor_value(p, "Speechcraft"), Err(SimError::UnknownActorValue(_))));
    w.add_item(p, id(SUIT), 1).unwrap();
    w.add_item(p, id(BULLETS), 100).unwrap();
    let e = w.encumbrance(p).unwrap();
    // 150 + 10 x 5; suit 15; ammo weightless outside Hardcore; caps weigh 0.
    assert_eq!((e.capacity, e.weight, e.over), (200.0, 15.0, false));
    // 15 + 23 x 8 + 1 = 200: at capacity, not over; one more unit is over.
    w.add_item(p, id(RIFLE), 23).unwrap();
    w.add_item(p, id(HAT), 1).unwrap();
    assert_eq!(w.encumbrance(p).unwrap().weight, 200.0);
    assert!(!w.encumbrance(p).unwrap().over);
    w.add_item(p, id(HAT), 1).unwrap();
    assert!(w.encumbrance(p).unwrap().over);
    w.remove_item(p, id(HAT), 2).unwrap();
    w.add_item(p, id(RIFLE), 1).unwrap();
    assert!(w.encumbrance(p).unwrap().over);
    w.set_actor_value(p, "strength", 10.0).unwrap();
    assert!(!w.encumbrance(p).unwrap().over);
    let mut hc = World::new(&f.forms, &f.refs, SimConfig { hardcore: true, ..SimConfig::default() });
    hc.add_item(p, id(BULLETS), 100).unwrap();
    assert!((hc.encumbrance(p).unwrap().weight - 10.0).abs() < 1e-4);
    // Hardcore turned on and off while the game runs (SetHardcore).
    w.set_hardcore(true);
    assert!(w.encumbrance(p).unwrap().weight > 200.0);
    w.set_hardcore(false);
    assert!(!w.encumbrance(p).unwrap().over);
    assert!(matches!(w.encumbrance(id(SAFE_REF)), Err(SimError::NoActorValues(_))));
}

#[test]
fn notes_chips_and_caravan_items_weigh_nothing() {
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let p = FormId::PLAYER_REF;
    w.add_item(p, id(SUIT), 1).unwrap();
    for item in [NOTE, CHIP, CMNY, CARD] {
        w.add_item(p, id(item), 50).unwrap();
    }
    let e = w.encumbrance(p).unwrap();
    assert_eq!((e.weight, e.unknown_weight), (15.0, vec![]));
}

#[test]
fn caravan_money_is_worth_its_absolute_value() {
    let f = fixture();
    let it = f.forms.item(id(CMNY)).unwrap();
    // The record has no Value field; its DATA is what the note is worth in caps.
    assert_eq!((it.value, it.money, it.worth()), (None, Some(40), Some(40)));
    let mut inv = fnv_sim::inventory::Inventory::default();
    inv.add(id(CMNY), 3);
    let t = inv.totals(&f.forms, false);
    assert_eq!((t.value, t.unknown_value.len()), (120, 0));
}

#[test]
fn placed_mines_disarm_into_their_weapon() {
    let f = fixture();
    assert_eq!(f.forms.item(id(MINE)).unwrap().projectile, Some(id(MINE_PROJ)));
    assert_eq!(f.forms.get(id(MINE_PROJ)).unwrap().data, BaseData::Projectile { item: Some(id(MINE)), can_be_disabled: true });
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let p = FormId::PLAYER_REF;
    let Outcome::Disarmed { item, placed: Some(left) } = w.activate(p, id(MINE_REF)).unwrap().outcome else { panic!("not disarmed") };
    assert_eq!(item, id(MINE));
    // The mine is gone; the disarmed one lies where it was, as a loose item.
    assert_eq!(w.item_count(p, id(MINE)).unwrap(), 0);
    assert_eq!(w.activate(p, id(MINE_REF)).unwrap().outcome, Outcome::Disabled);
    let (mine, loose) = (w.peek(id(MINE_REF)).unwrap().clone(), w.peek(left).unwrap().clone());
    assert_eq!((loose.base, loose.pos, loose.cell), (id(MINE), mine.pos, mine.cell));
    assert!(matches!(w.activate(p, left).unwrap().outcome, Outcome::PickedUp { count: 1, .. }));
    assert_eq!(w.item_count(p, id(MINE)).unwrap(), 1);
}

#[test]
fn scripted_activation_leaves_placed_mines_alone() {
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let p = FormId::PLAYER_REF;
    for _ in 0..2 {
        assert_eq!(w.activate_scripted(p, id(MINE_REF)).unwrap().outcome, Outcome::NoDefault { sig: "PROJ".into() });
    }
    assert_eq!(w.item_count(p, id(MINE)).unwrap(), 0);
    assert!(w.is_enabled(id(MINE_REF)).unwrap());
    // The player's own activation still disarms it afterwards.
    assert!(matches!(w.activate(p, id(MINE_REF)).unwrap().outcome, Outcome::Disarmed { placed: Some(_), .. }));
}

#[test]
fn respawning_containers_refill_after_the_delay() {
    // The cell respawn alone: the safe is also a merchant container in the fixture, and
    // merchant restock days would refill it too.
    let mut f = fixture();
    f.refs.merchant_containers.clear();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let p = FormId::PLAYER_REF;
    w.unlock(id(SAFE_REF)).unwrap();
    w.transfer_all(id(SAFE_REF), p).unwrap();
    w.add_item(id(SAFE_REF), id(PISTOL), 1).unwrap(); // left behind by the player
    let cell = id(CELL_A);
    // Not long enough: nothing resets.
    w.detach_cell(cell).unwrap();
    w.advance_hours(71.0).unwrap();
    assert!(w.attach_cell(cell).unwrap().is_empty());
    assert_eq!(w.item_count(id(SAFE_REF), id(CAPS)).unwrap(), 0);
    // A second absence long enough: the safe (Respawns flag) is back to its authored
    // state, the pistol is gone, the lock is back.
    w.detach_cell(cell).unwrap();
    w.advance_hours(72.0).unwrap();
    assert_eq!(w.attach_cell(cell).unwrap(), vec![id(SAFE_REF)]);
    assert!(w.peek(id(SAFE_REF)).is_none());
    assert_eq!(w.item_count(id(SAFE_REF), id(CAPS)).unwrap(), 3);
    assert_eq!(w.item_count(id(SAFE_REF), id(PISTOL)).unwrap(), 0);
    assert!(w.peek(id(SAFE_REF)).unwrap().lock.unwrap().locked);
    // Attaching a cell that was never detached does nothing; the config is honoured.
    assert!(w.attach_cell(id(CELL_B)).unwrap().is_empty());
    let mut quick = World::new(&f.forms, &f.refs, SimConfig { respawn_hours: Some(1.0), ..SimConfig::default() });
    quick.get(id(SAFE_REF)).unwrap();
    quick.detach_cell(cell).unwrap();
    quick.advance_hours(1.0).unwrap();
    assert_eq!(quick.attach_cell(cell).unwrap(), vec![id(SAFE_REF)]);
    // Detached cells survive a snapshot.
    w.detach_cell(cell).unwrap();
    let snap = w.snapshot();
    assert!(snap.detached_cells.contains_key(&cell));
}

#[test]
fn leveled_sampling_histograms() {
    let f = fixture();
    let cases: fnv_sim::sample::Cases = serde_json::from_str(
        r#"{"format": "fnv.sim.cases/1", "seed": 4, "cases": [
            {"label": "loot at 10", "list": "TestLoot", "level": 10, "count": 2, "samples": 50},
            {"label": "never", "list": "0x00000111", "samples": 10},
            {"label": "not a list", "list": "TestCaps", "samples": 10},
            {"label": "loop", "list": "TestLoop", "samples": 10},
            {"label": "odds 0", "list": "TestOddsList", "globals": {"TestOdds": 0}, "samples": 10},
            {"label": "bad global", "list": "TestOddsList", "globals": {"TestCaps": 0}, "samples": 10}]}"#,
    )
    .unwrap();
    let out = fnv_sim::sample::run(&f.forms, &cases).unwrap();
    assert_eq!(out["format"], "fnv.sim.samples/1");
    let c = out["cases"].as_array().unwrap();
    assert_eq!(c[0]["list"], "0x00000110");
    assert_eq!(c[0]["outcomes"], serde_json::json!({"0x00000101 x20": 50}));
    assert_eq!(c[1]["outcomes"], serde_json::json!({"nothing": 10}));
    assert!(c[2]["error"].as_str().unwrap().contains("not a leveled item list"));
    assert!(c[3]["error"].as_str().unwrap().contains("nesting"));
    assert_eq!(c[4]["outcomes"], serde_json::json!({"0x00000100 x1": 10}));
    assert_eq!(c[4]["globals"], serde_json::json!({"TestOdds": 0.0}));
    assert!(c[5]["error"].as_str().unwrap().contains("not a global"));
    assert_eq!(fnv_sim::sample::run(&f.forms, &cases).unwrap(), out);
}

#[test]
fn teleporting_into_a_worldspace_lands_in_the_grid_cell() {
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let p = FormId::PLAYER_REF;
    let r = w.activate(p, id(IN_DOOR)).unwrap();
    // The destination door is stored in the persistent cell; the player lands in the
    // exterior cell containing the destination position.
    assert_eq!(
        r.outcome,
        Outcome::Teleported {
            to_door: id(EXT_DOOR),
            cell: Some(id(WORLD_CELL)),
            pos: [-73162.9, 1312.4, 8686.4],
            rot: [0.0, 0.0, 1.5]
        }
    );
    assert_eq!(w.peek(p).unwrap().cell, Some(id(WORLD_CELL)));
    // Back inside: an interior door keeps its own cell.
    w.activate(p, id(EXT_DOOR)).unwrap();
    assert_eq!(w.peek(p).unwrap().cell, Some(id(CELL_B)));
}

#[test]
fn teleporting_into_another_worldspace_moves_cell_and_worldspace_together() {
    let f = fixture();
    assert_eq!(f.refs.world_of(id(WORLD2_CELL)), Some(id(WORLD2)));
    assert_eq!(f.refs.world_of(id(CELL_B)), None);
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let p = FormId::PLAYER_REF;
    // Out of the interior into the first worldspace.
    w.activate(p, id(IN_DOOR)).unwrap();
    assert_eq!(w.worldspace_of(p), Some(id(WORLD)));
    // Through the gate: the destination door is persistent in the other worldspace; the
    // player lands in that worldspace's exterior cell containing the destination.
    let r = w.activate(p, id(GATE_OUT)).unwrap();
    assert_eq!(
        r.outcome,
        Outcome::Teleported { to_door: id(GATE_IN), cell: Some(id(WORLD2_CELL)), pos: [8288.0, 6896.0, 1008.0], rot: [0.0, 0.0, 1.5] }
    );
    let ps = w.peek(p).unwrap();
    assert_eq!((ps.cell, ps.pos), (Some(id(WORLD2_CELL)), [8288.0, 6896.0, 1008.0]));
    assert_eq!(w.worldspace_of(p), Some(id(WORLD2)));
    // The game moves the player on (the physics arrival, walking): cell and position
    // as the caller writes them, still in that worldspace.
    let s = w.get(p).unwrap();
    s.pos = [8300.0, 6900.0, 1010.5];
    // A snapshot taken here resumes in the same worldspace, cell and position.
    let text = serde_json::to_string(&w.snapshot()).unwrap();
    let mut resumed = World::restore(&f.forms, &f.refs, &serde_json::from_str(&text).unwrap()).unwrap();
    let rs = resumed.peek(p).unwrap();
    assert_eq!((rs.cell, rs.pos), (Some(id(WORLD2_CELL)), [8300.0, 6900.0, 1010.5]));
    assert_eq!(resumed.worldspace_of(p), Some(id(WORLD2)));
    // The door back returns to the first worldspace's grid cell (-18, 0).
    resumed.activate(p, id(GATE_IN)).unwrap();
    let rs = resumed.peek(p).unwrap();
    assert_eq!((rs.cell, rs.pos), (Some(id(WORLD_CELL)), [-73250.0, 1450.0, 8690.0]));
    assert_eq!(resumed.worldspace_of(p), Some(id(WORLD)));
    // Indoors there is no worldspace.
    resumed.activate(p, id(EXT_DOOR)).unwrap();
    assert_eq!(resumed.worldspace_of(p), None);
    // References not yet materialised answer from their placement.
    assert_eq!(resumed.worldspace_of(id(GATE_IN)), Some(id(WORLD2)));
    assert_eq!(resumed.worldspace_of(id(SAFE_REF)), None);
}

#[test]
fn respawn_delay_comes_from_config_then_setting_then_default() {
    use fnv_sim::world::DEFAULT_RESPAWN_HOURS;
    let f = fixture();
    // The fixture defines no iHoursToRespawnCell: the default applies.
    assert_eq!(World::new(&f.forms, &f.refs, SimConfig::default()).respawn_hours(), DEFAULT_RESPAWN_HOURS);
    let cfg = SimConfig { respawn_hours: Some(5.0), ..SimConfig::default() };
    assert_eq!(World::new(&f.forms, &f.refs, cfg).respawn_hours(), 5.0);
    // A plugin setting is used when the config does not override it.
    let mut forms = FormStore::default();
    forms.settings.insert("ihourstorespawncell".into(), fnv_sim::settings::Setting::Int(24));
    let refs = RefTable::default();
    assert_eq!(World::new(&forms, &refs, SimConfig::default()).respawn_hours(), 24.0);
    // Older snapshots stored a plain number.
    let old: SimConfig = serde_json::from_str(r#"{"seed": 0, "player_level": 1, "respawn_hours": 72.0}"#).unwrap();
    assert_eq!(old.respawn_hours, Some(72.0));
}

#[test]
fn merchant_restock_days_are_0_and_3_modulo_7() {
    use fnv_sim::world::is_merchant_restock_day;
    // Every day the in-game captures restocked on, and every day they passed over.
    for d in [7, 10, 14, 17, 21, 24, 28] {
        assert!(is_merchant_restock_day(d), "{d}");
    }
    for d in [8, 9, 13, 15, 16, 18, 19, 20, 22, 23, 25, 26, 27] {
        assert!(!is_merchant_restock_day(d), "{d}");
    }
    assert!(is_merchant_restock_day(0) && is_merchant_restock_day(-4));
}

#[test]
fn merchant_containers_restock_on_restock_days() {
    // CHAIR_REF names SAFE_REF in XMRC: the safe is a merchant container, and its base
    // has the Respawns flag. The fixture's clock starts at GameDaysPassed 5.0; restock
    // days are 7, 10, 14, 17, 21, 24, ...
    let mut f = fixture();
    let p = FormId::PLAYER_REF;
    let safe = id(SAFE_REF);
    {
        let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
        assert_eq!(w.item_count(safe, id(CAPS)).unwrap(), 3);
        w.unlock(safe).unwrap();
        w.transfer_all(safe, p).unwrap();
        w.add_item(safe, id(AID), 1).unwrap(); // "sold" to the merchant
        // Midnight 6 is no restock day.
        w.advance_hours(47.0).unwrap();
        assert_eq!((w.item_count(safe, id(CAPS)).unwrap(), w.item_count(safe, id(AID)).unwrap()), (0, 1));
        // Midnight 7: restocked, contents replaced, without any cell event or access.
        w.advance_hours(1.0).unwrap();
        assert_eq!(w.peek(safe).unwrap().last_restock, Some(7.0));
        assert_eq!((w.item_count(safe, id(CAPS)).unwrap(), w.item_count(safe, id(AID)).unwrap()), (3, 0));
        // Emptied at once: 3 days to the next restock (10), then 4 (14).
        w.transfer_all(safe, p).unwrap();
        w.advance_hours(71.0).unwrap();
        assert_eq!(w.item_count(safe, id(CAPS)).unwrap(), 0);
        w.advance_hours(1.0).unwrap();
        assert_eq!(w.item_count(safe, id(CAPS)).unwrap(), 3);
        w.transfer_all(safe, p).unwrap();
        w.advance_hours(72.0).unwrap();
        assert_eq!(w.item_count(safe, id(CAPS)).unwrap(), 0);
        w.advance_hours(24.0).unwrap();
        assert_eq!(w.item_count(safe, id(CAPS)).unwrap(), 3);
        // The schedule does not follow accesses or the last restock: emptied a day
        // before a restock day, the box is full again a day later.
        w.advance_hours(48.0).unwrap(); // day 16
        w.transfer_all(safe, p).unwrap();
        assert!(matches!(w.activate(p, safe).unwrap().outcome, Outcome::OpenedContainer { .. }));
        w.advance_hours(24.0).unwrap();
        assert_eq!(w.item_count(safe, id(CAPS)).unwrap(), 3);
        assert_eq!(w.peek(safe).unwrap().last_restock, Some(17.0));
        // Several restock days in one advance (21, 24) restock once, at the last.
        w.advance_hours(24.0 * 9.0).unwrap();
        assert_eq!(w.peek(safe).unwrap().last_restock, Some(24.0));
        assert_eq!(w.item_count(id(GUN_REF), id(CAPS)), Err(SimError::NoInventory(id(GUN_REF))));
        // The restock time is part of the snapshot, and a restored world continues the
        // same way.
        w.transfer_all(safe, p).unwrap();
        let snap = w.snapshot();
        assert_eq!(snap.refs[&safe].last_restock, Some(24.0));
        let mut r = World::restore(&f.forms, &f.refs, &snap).unwrap();
        w.advance_hours(24.0 * 4.0).unwrap();
        r.advance_hours(24.0 * 4.0).unwrap();
        assert_eq!(r.snapshot(), w.snapshot());
        assert_eq!(r.item_count(safe, id(CAPS)).unwrap(), 3);
    }
    // A merchant container whose base lacks the Respawns flag never restocks.
    if let BaseData::Container(c) = &mut f.forms.bases.get_mut(&id(SAFE)).unwrap().data {
        c.respawns = false;
    }
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    w.unlock(safe).unwrap();
    w.transfer_all(safe, p).unwrap();
    w.advance_hours(24.0 * 10.0).unwrap();
    assert_eq!(w.item_count(safe, id(CAPS)).unwrap(), 0);
    assert_eq!(w.peek(safe).unwrap().last_restock, None);
}

#[test]
fn get_locked_reads_zero_or_one() {
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let p = FormId::PLAYER_REF;
    let safe = id(SAFE_REF);
    assert_eq!(w.lock_status(safe).unwrap(), 1);
    assert_eq!(w.lock_status(id(CHAIR_REF)).unwrap(), 0, "no lock data");
    // Level 255 still reads 1 in game, not the wiki's 2 (simulation.md §5).
    w.lock(safe, Some(255)).unwrap();
    assert_eq!(w.lock_status(safe).unwrap(), 1);
    assert_eq!(w.activate(p, safe).unwrap().outcome, Outcome::Locked { level: 255 });
    // The key opens it for good.
    w.add_item(p, id(KEY), 1).unwrap();
    assert_eq!(w.activate(p, safe).unwrap().unlocked_with, Some(id(KEY)));
    assert_eq!(w.lock_status(safe).unwrap(), 0);
    // Lock without a level keeps the old one.
    w.lock(safe, None).unwrap();
    assert_eq!(w.peek(safe).unwrap().lock.unwrap().level, 255);
}

#[test]
fn drops_land_ahead_of_and_above_the_holder() {
    use fnv_sim::world::{DROP_ABOVE, DROP_AHEAD};
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let p = FormId::PLAYER_REF;
    w.add_item(p, id(STIMPAK), 2).unwrap();
    // Facing north (z rotation 0): ahead is +Y.
    let r = w.drop_item(p, id(STIMPAK), 1).unwrap()[0];
    assert_eq!(w.peek(r).unwrap().pos, [0.0, DROP_AHEAD, DROP_ABOVE]);
    // Facing east (z = pi/2, clockwise from north): ahead is +X.
    w.get(p).unwrap().rot[2] = std::f32::consts::FRAC_PI_2;
    w.get(p).unwrap().pos = [10.0, 20.0, 30.0];
    let r = w.drop_item(p, id(STIMPAK), 1).unwrap()[0];
    let pos = w.peek(r).unwrap().pos;
    assert!((pos[0] - (10.0 + DROP_AHEAD)).abs() < 1e-3 && (pos[1] - 20.0).abs() < 1e-3, "{pos:?}");
    assert_eq!(pos[2], 30.0 + DROP_ABOVE);
}

#[test]
fn hand_drops_land_at_chest_height_in_a_ring() {
    use fnv_sim::world::{player_drop_offset, DROP_AHEAD, PLAYER_DROP_ABOVE, PLAYER_DROP_RING};
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let p = FormId::PLAYER_REF;
    w.add_item(p, id(STIMPAK), 20).unwrap();
    // Facing north: the first lands ahead (+Y) at chest height, the next to its right
    // (towards +X), the next to its left, ..., the eighth behind.
    let mut seen = Vec::new();
    for _ in 0..PLAYER_DROP_RING {
        let r = w.player_drop_item(id(STIMPAK), 1).unwrap()[0];
        seen.push(w.peek(r).unwrap().pos);
    }
    assert_eq!(seen[0], [0.0, DROP_AHEAD, PLAYER_DROP_ABOVE]);
    assert!(seen[1][0] > 1.0 && seen[2][0] < -1.0, "{seen:?}");
    assert!((seen[7][1] + DROP_AHEAD).abs() < 1e-3 && seen[7][0].abs() < 1e-3, "{seen:?}");
    for (i, a) in seen.iter().enumerate() {
        for b in &seen[i + 1..] {
            assert!((a[0] - b[0]).hypot(a[1] - b[1]) > 30.0, "{seen:?}");
        }
    }
    // The ring is full: the next goes in the second, farther out, straight ahead.
    let r = w.player_drop_item(id(STIMPAK), 1).unwrap()[0];
    assert_eq!(w.peek(r).unwrap().pos[..2], player_drop_offset(0.0, PLAYER_DROP_RING));
    assert!(w.peek(r).unwrap().pos[1] > DROP_AHEAD + 1.0);
    // Taking the first back frees its place.
    let first = w.created().next().unwrap().0;
    w.activate(p, first).unwrap();
    let r = w.player_drop_item(id(STIMPAK), 1).unwrap()[0];
    assert_eq!(w.peek(r).unwrap().pos, [0.0, DROP_AHEAD, PLAYER_DROP_ABOVE]);
}

#[test]
fn created_lists_drops_taken_ones_included() {
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let p = FormId::PLAYER_REF;
    w.add_item(p, id(STIMPAK), 2).unwrap();
    assert_eq!(w.created().count(), 0);
    let a = w.drop_item(p, id(STIMPAK), 1).unwrap()[0];
    let b = w.drop_item(p, id(STIMPAK), 1).unwrap()[0];
    w.activate(p, a).unwrap();
    let listed: Vec<(FormId, bool)> = w.created().map(|(id, s)| (id, s.deleted())).collect();
    assert_eq!(listed, vec![(a, true), (b, false)]);
    // The player (materialised, below the created range) is not listed.
    assert!(w.peek(p).is_some());
}

const CREATED_SCRIPT: &str = r#"{
  "format": "fnv.sim.actions/1",
  "actions": [
    {"op": "get_locked", "of": "TestSafeRef", "label": "locked"},
    {"op": "activate", "actor": "player", "target": "TestSafeRef", "label": "no key"},
    {"op": "add_item", "to": "player", "item": "TestKey", "count": 1},
    {"op": "activate", "actor": "player", "target": "TestSafeRef", "label": "key"},
    {"op": "get_locked", "of": "TestSafeRef", "label": "unlocked"},
    {"op": "activate", "actor": "player", "target": "created:TestStim", "label": "none yet"},
    {"op": "add_item", "to": "player", "item": "TestStim", "count": 2},
    {"op": "drop_item", "from": "player", "item": "TestStim", "count": 1},
    {"op": "drop_item", "from": "player", "item": "TestStim", "count": 1},
    {"op": "get_pos", "of": "created:TestStim", "label": "newest"},
    {"op": "activate", "actor": "player", "target": "created:TestStim", "label": "item"},
    {"op": "get_pos", "of": "created:0x00000100", "label": "older"}
  ]
}"#;

#[test]
fn created_names_and_labelled_activations() {
    let f = fixture();
    let s: Script = serde_json::from_str(CREATED_SCRIPT).unwrap();
    let mut w = World::new(&f.forms, &f.refs, actions::config(&s));
    let t = actions::run(&mut w, &s).unwrap();
    let obs: Vec<_> = t["observations"].as_array().unwrap().iter().map(|o| {
        let mut o = o.clone();
        o.as_object_mut().unwrap().remove("step");
        o
    }).collect();
    use serde_json::json;
    assert_eq!(obs[0], json!({"label": "locked", "locked": 1}));
    assert_eq!(obs[1], json!({"label": "no key", "opened": false}));
    assert_eq!(obs[2], json!({"label": "key", "opened": true}));
    assert_eq!(obs[3], json!({"label": "unlocked", "locked": 0}));
    assert!(obs[4]["error"].as_str().unwrap().contains("no created reference"));
    // Two drops: the newest (second) reference is named first; once it is taken, the
    // older one.
    assert_eq!(t["steps"][9]["result"]["cell"], serde_json::Value::Null);
    assert_eq!(t["steps"][8]["result"]["references"], json!(["0xFF000001"]));
    assert_eq!(obs[6], json!({"label": "item", "opened": false}));
    assert_eq!(t["steps"][10]["result"]["result"], "picked_up");
    assert_eq!(t["final"]["0xFF000001"]["deleted"], true);
    assert_eq!(t["final"]["0xFF000000"]["deleted"], false);
    assert!(obs[7].get("error").is_none(), "{}", obs[7]);
}

#[test]
fn scripted_activation_takes_one_item_of_a_stack() {
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let p = FormId::PLAYER_REF;
    let stack = id(STIM_REF); // XCNT 3
    let r = w.activate_scripted(p, stack).unwrap();
    assert_eq!(r.outcome, Outcome::PickedUp { item: id(STIMPAK), count: 1, owner: None });
    assert_eq!(w.peek(stack).unwrap().count, 1, "the rest of the stack is lost");
    w.activate_scripted(p, stack).unwrap();
    assert_eq!(w.item_count(p, id(STIMPAK)).unwrap(), 2);
    // A dropped stack likewise.
    w.add_item(p, id(STIMPAK), 1).unwrap();
    let dropped = w.drop_item(p, id(STIMPAK), 3).unwrap()[0];
    assert_eq!(w.peek(dropped).unwrap().count, 3);
    w.activate_scripted(p, dropped).unwrap();
    assert_eq!(w.item_count(p, id(STIMPAK)).unwrap(), 1);
    // The player's own activation takes the whole stack (hypothesis).
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    assert_eq!(w.activate(p, stack).unwrap().outcome, Outcome::PickedUp { item: id(STIMPAK), count: 3, owner: None });
}

#[test]
fn non_playable_armour_is_flagged() {
    let f = fixture();
    assert!(f.forms.item(id(GLOVE)).unwrap().non_playable);
    assert!(!f.forms.item(id(SUIT)).unwrap().non_playable);
    assert!(!f.forms.item(id(PISTOL)).unwrap().non_playable);
}

#[test]
fn non_playable_weapons_and_ammo_are_flagged_and_leave_with_remove_all_items() {
    let f = fixture();
    let gun = f.forms.item(id(COMPANION_GUN)).unwrap();
    assert!(gun.non_playable && gun.cant_drop && !gun.non_playable_biped());
    assert!(gun.listed(true) && !gun.listed(false), "the player's own inventory shows it, others' do not");
    let ammo = f.forms.item(id(COMPANION_AMMO)).unwrap();
    assert!(ammo.non_playable && !ammo.cant_drop && ammo.listed(true) && !ammo.listed(false));
    let pistol = f.forms.item(id(PISTOL)).unwrap();
    assert!(!pistol.non_playable && !pistol.cant_drop && pistol.listed(false));
    let glove = f.forms.item(id(GLOVE)).unwrap();
    assert!(glove.non_playable_biped() && !glove.listed(true) && !glove.listed(false));
    // GECK wiki RemoveAllItems keeps only unplayable biped items: these move.
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let p = FormId::PLAYER_REF;
    w.add_item(p, id(COMPANION_GUN), 1).unwrap();
    w.add_item(p, id(COMPANION_AMMO), 5).unwrap();
    let removed = w.remove_all_items(p).unwrap();
    assert!(removed.contains(&(id(COMPANION_GUN), 1)) && removed.contains(&(id(COMPANION_AMMO), 5)));
    assert_eq!(w.item_count(p, id(GLOVE)).unwrap(), 1);
}

#[test]
fn the_player_starts_wearing_non_playable_armour_and_keeps_it() {
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let p = FormId::PLAYER_REF;
    assert!(w.inventory_mut(p).unwrap().is_equipped(id(GLOVE)));
    assert!(!w.inventory_mut(p).unwrap().is_equipped(id(CAPS)));
    // RemoveAllItems takes everything else and leaves the glove worn.
    w.add_item(p, id(SUIT), 1).unwrap();
    w.equip(p, id(SUIT)).unwrap();
    let removed = w.remove_all_items(p).unwrap();
    assert!(removed.iter().all(|&(i, _)| i != id(GLOVE)));
    assert_eq!(w.item_count(p, id(GLOVE)).unwrap(), 1);
    assert_eq!(w.item_count(p, id(SUIT)).unwrap(), 0);
    assert_eq!(w.item_count(p, id(CAPS)).unwrap(), 0);
    assert!(w.inventory_mut(p).unwrap().is_equipped(id(GLOVE)));
}

#[test]
fn remove_all_items_leaves_the_players_quest_items() {
    // GECK wiki RemoveAllItems: on the player, quest items stay; moved or not.
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let p = FormId::PLAYER_REF;
    w.add_item(p, id(SUIT), 1).unwrap();
    w.add_item(p, id(PISTOL), 1).unwrap();
    w.set_quest_object(id(SUIT), true);
    let removed = w.remove_all_items(p).unwrap();
    assert!(removed.iter().all(|&(i, _)| i != id(SUIT)));
    assert_eq!(w.item_count(p, id(SUIT)).unwrap(), 1);
    assert_eq!(w.item_count(p, id(PISTOL)).unwrap(), 0);
    w.add_item(p, id(PISTOL), 1).unwrap();
    let moved = w.transfer_all_owned(p, id(SAFE_REF), true).unwrap();
    assert!(moved.iter().all(|&(i, _)| i != id(SUIT)));
    assert_eq!(w.item_count(p, id(SUIT)).unwrap(), 1);
    assert_eq!(w.item_count(id(SAFE_REF), id(PISTOL)).unwrap(), 1);
}

#[test]
fn an_npc_given_an_item_equips_its_best_weapon_and_armour() {
    // GECK wiki AddItem notes: AddItem on an NPC makes it review its inventory and equip
    // the best armour and weapon it has (playtest list 7 item 12: Trudy's shotgun).
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let g = w.place(id(GUARD), None, [0.0; 3], [0.0; 3], 1).unwrap();
    // Unarmed and undressed by a script, it takes both up again once given something.
    w.unequip(g, id(LONG_GUN)).unwrap();
    w.unequip(g, id(SUIT)).unwrap();
    assert!(w.review_equipment(FormId::PLAYER_REF).is_empty());
    w.add_item(g, id(CAPS), 5).unwrap();
    let changed = w.review_equipment(g);
    let inv = w.inventory_mut(g).unwrap();
    assert!(inv.is_equipped(id(LONG_GUN)) && inv.is_equipped(id(SUIT)));
    assert_eq!(changed.len(), 2);
    // Nothing more to change: a second review does nothing.
    assert!(w.review_equipment(g).is_empty());
}

#[test]
fn an_npc_starts_with_its_outfit_and_weapon_equipped() {
    // The engine's wearing and arming rules (World::equip_default): the helmet (first
    // carried, no DT anywhere) takes Head|Hair, so the hat (Hair) stays in the pack; the
    // suit has a slot of its own; of the weapons only the long gun can be drawn. A
    // created actor is outfitted at once.
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let g = w.place(id(GUARD), None, [0.0; 3], [0.0; 3], 1).unwrap();
    assert!(w.get(g).unwrap().outfitted);
    let inv = w.inventory_mut(g).unwrap();
    // The Player Only gun hits harder but is no NPC's choice (weapon-data-fields.md §1).
    assert!(f.forms.item(id(PLAYER_GUN)).is_some_and(|it| it.drawable && it.not_for_npcs));
    assert!(!f.forms.item(id(LONG_GUN)).unwrap().not_for_npcs);
    // The throwing spears hit harder too, but a weapon held in the hands comes first.
    assert!(f.forms.item(id(THROWN_SPEAR)).is_some_and(|it| it.drawable && it.thrown && !it.not_for_npcs));
    assert!(!f.forms.item(id(LONG_GUN)).unwrap().thrown);
    for (item, worn) in [(HELMET, true), (HAT, false), (SUIT, true), (LONG_GUN, true), (PISTOL, false), (RIFLE, false), (MINE, false), (PLAYER_GUN, false), (THROWN_SPEAR, false)] {
        assert_eq!(inv.is_equipped(id(item)), worn, "{item:#X}");
    }
    assert_eq!(inv.count(id(MINE)), 2);
    // Once only: what is taken off later stays off.
    w.unequip(g, id(SUIT)).unwrap();
    assert!(!w.outfit(g).unwrap());
    assert!(!w.inventory_mut(g).unwrap().is_equipped(id(SUIT)));
    // ResetInventory rolls again and an outfitted actor dresses again.
    assert!(w.reset_inventory(g).unwrap());
    assert!(w.inventory_mut(g).unwrap().is_equipped(id(SUIT)));
    // A container's items are never equipped; the player is never outfitted this way.
    let s = w.place(id(SAFE), None, [0.0; 3], [0.0; 3], 1).unwrap();
    assert!(w.inventory_mut(s).unwrap().equipped().is_empty());
    assert!(!w.get(s).unwrap().outfitted);
    assert!(!w.outfit(FormId::PLAYER_REF).unwrap());
}

#[test]
fn an_npcs_equipment_is_what_it_has_equipped_once_outfitted() {
    // World::equipment, what the engine draws: the equipped items once outfitted (EquipItem
    // and UnequipItem change it), before that the items the outfitting will equip.
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let g = w.place(id(GUARD), None, [0.0; 3], [0.0; 3], 1).unwrap();
    let set = |v: Option<Vec<FormId>>| -> Vec<u32> {
        let mut v: Vec<u32> = v.unwrap().into_iter().map(|i| i.0).collect();
        v.sort();
        v
    };
    let mut dressed = vec![HELMET, SUIT, LONG_GUN];
    dressed.sort();
    assert_eq!(set(w.equipment(g)), dressed);
    w.unequip(g, id(SUIT)).unwrap();
    w.equip(g, id(PISTOL)).unwrap();
    let mut now = vec![HELMET, PISTOL];
    now.sort();
    assert_eq!(set(w.equipment(g)), now);
    // Not outfitted yet: what the outfitting would equip, whatever the flags say.
    w.get(g).unwrap().outfitted = false;
    assert_eq!(set(w.equipment(g)), dressed);
    // A container is no actor.
    let s = w.place(id(SAFE), None, [0.0; 3], [0.0; 3], 1).unwrap();
    assert!(w.equipment(s).is_none());
    // The player, never outfitted: only what it has equipped, not the armour it carries
    // (issue #1364).
    let p = FormId::PLAYER_REF;
    w.add_item(p, id(SUIT), 1).unwrap();
    assert!(!w.equipment(p).unwrap().contains(&id(SUIT)));
    w.equip(p, id(SUIT)).unwrap();
    assert!(w.equipment(p).unwrap().contains(&id(SUIT)));
}

// ---- quest items ----------------------------------------------------------------

#[test]
fn quest_item_flag_comes_from_the_record_header() {
    let f = fixture();
    assert!(f.forms.issues.is_empty(), "{:?}", f.forms.issues.to_json());
    assert!(f.forms.is_quest_item(id(QUEST_KEY)));
    assert!(!f.forms.is_quest_item(id(KEY)));
    assert!(!f.forms.is_quest_item(id(STIMPAK)));
    // The schema names the bit per record type; a type it gives no name stays false.
    let s = schema();
    assert_eq!(s.header_flag(b"KEYM", fnv_sim::forms::QUEST_ITEM_FLAG), Some(0x400));
    assert_eq!(s.header_flag(b"NOTE", fnv_sim::forms::QUEST_ITEM_FLAG), None);
}

#[test]
fn the_player_cannot_drop_a_quest_item_by_hand() {
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let p = FormId::PLAYER_REF;
    w.add_item(p, id(QUEST_KEY), 3).unwrap();
    w.add_item(p, id(STIMPAK), 1).unwrap();
    assert_eq!(w.player_drop_item(id(QUEST_KEY), 1), Err(SimError::QuestItem(id(QUEST_KEY))));
    assert_eq!(w.item_count(p, id(QUEST_KEY)).unwrap(), 3);
    assert_eq!(w.created().count(), 0);
    // A bad count is still a bad count; other items drop as before.
    assert_eq!(w.player_drop_item(id(QUEST_KEY), 0), Err(SimError::BadCount(0)));
    assert_eq!(w.player_drop_item(id(STIMPAK), 1).unwrap().len(), 1);
    // A script's drop ignores the flag [observed: scripted player.Drop in the game].
    assert_eq!(w.drop_item(p, id(QUEST_KEY), 1).unwrap().len(), 1);
    assert_eq!(w.item_count(p, id(QUEST_KEY)).unwrap(), 2);
    // Storing it in a container is allowed (see crates/fnv-sim/src/world.rs), and the container may drop it.
    assert_eq!(w.transfer(p, id(SAFE_REF), id(QUEST_KEY), 1).unwrap(), 1);
    assert_eq!(w.drop_item(id(SAFE_REF), id(QUEST_KEY), 1).unwrap().len(), 1);
    // The flag is the error's message.
    assert_eq!(SimError::QuestItem(id(QUEST_KEY)).to_string(), "0x0000017D is a quest item: the player cannot drop it");
}

#[test]
fn set_quest_object_overrides_the_record_flag_and_is_saved() {
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let p = FormId::PLAYER_REF;
    w.add_item(p, id(STIMPAK), 1).unwrap();
    w.add_item(p, id(QUEST_KEY), 1).unwrap();
    w.set_quest_object(id(STIMPAK), true);
    w.set_quest_object(id(QUEST_KEY), false);
    assert!(w.is_quest_item(id(STIMPAK)) && !w.is_quest_item(id(QUEST_KEY)));
    assert_eq!(w.player_drop_item(id(STIMPAK), 1), Err(SimError::QuestItem(id(STIMPAK))));
    let snap = w.snapshot();
    assert_eq!(snap.quest_objects.len(), 2);
    let mut back = World::restore(&f.forms, &f.refs, &serde_json::from_str(&serde_json::to_string(&snap).unwrap()).unwrap()).unwrap();
    assert!(back.is_quest_item(id(STIMPAK)));
    assert_eq!(back.player_drop_item(id(QUEST_KEY), 1).unwrap().len(), 1);
    // A world with no overrides writes no field (older snapshots stay byte-identical).
    let plain = World::new(&f.forms, &f.refs, SimConfig::default()).snapshot();
    assert!(!serde_json::to_string(&plain).unwrap().contains("quest_objects"));
}

#[test]
fn placed_ammo_without_a_count_holds_its_clip_rounds() {
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let p = FormId::PLAYER_REF;
    assert_eq!(f.forms.item(id(BULLETS)).unwrap().placed_count, Some(10));
    assert_eq!(f.forms.placed_count(id(BULLETS), None), 10);
    // XCNT wins; other items without one are one unit.
    assert_eq!(f.forms.placed_count(id(BULLETS), Some(3)), 3);
    assert_eq!(f.forms.placed_count(id(PISTOL), None), 1);
    let before = w.item_count(p, id(BULLETS)).unwrap();
    w.activate(p, id(ROUNDS_REF)).unwrap();
    assert_eq!(w.item_count(p, id(BULLETS)).unwrap(), before + 10);
}

/// Skill books (docs/spec-notes/skill-books.md §2, §4): the stimpak stands in for a book.
fn as_book(f: &mut Fixture, teaches: Option<u16>, cant_be_taken: bool) {
    let b = f.forms.bases.get_mut(&id(STIMPAK)).unwrap();
    match &mut b.data {
        BaseData::Item(i) => {
            i.kind = ItemKind::Book;
            i.teaches = teaches;
            i.cant_be_taken = cant_be_taken;
        }
        other => panic!("stimpak: {other:?}"),
    }
}

#[test]
fn a_skill_book_is_used_up_and_a_plain_book_is_not() {
    let mut f = fixture();
    as_book(&mut f, Some(41), false);
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let p = FormId::PLAYER_REF;
    w.add_item(p, id(STIMPAK), 2).unwrap();
    assert!(w.equip(p, id(STIMPAK)).unwrap().consumed);
    assert_eq!(w.item_count(p, id(STIMPAK)).unwrap(), 1);
    let mut f = fixture();
    as_book(&mut f, None, false);
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    w.add_item(p, id(STIMPAK), 1).unwrap();
    assert_eq!(w.equip(p, id(STIMPAK)), Err(SimError::NotEquippable(id(STIMPAK))));
    assert_eq!(w.item_count(p, id(STIMPAK)).unwrap(), 1);
}

#[test]
fn a_book_that_cant_be_taken_stays_unless_a_script_activates_it() {
    let mut f = fixture();
    as_book(&mut f, None, true);
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let p = FormId::PLAYER_REF;
    assert!(matches!(w.activate(p, id(STIM_REF)).unwrap().outcome, Outcome::NoDefault { .. }));
    assert_eq!(w.item_count(p, id(STIMPAK)).unwrap(), 0);
    assert!(matches!(w.activate_scripted(p, id(STIM_REF)).unwrap().outcome, Outcome::PickedUp { .. }));
}

/// A created reference picked up is saved as its FormID and base only (#1002): after the
/// load a held reference to it still answers that it is deleted, its base is known, its
/// FormID is not given again, and saves no longer grow by a whole entry per drop and
/// pick-up.
#[test]
fn deleted_created_references_are_pruned_from_saves() {
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let p = FormId::PLAYER_REF;
    let cell = Some(id(CELL_A));
    let held = w.place(id(STIMPAK), cell, [1.0, 2.0, 3.0], [0.0; 3], 1).unwrap();
    // Both dropped by the same dead holder (`dropped_beside`); one is then taken.
    let holder = id(SAFE_REF);
    w.get(held).unwrap().dropped_by = Some(holder);
    assert!(matches!(w.activate(p, held).unwrap().outcome, Outcome::PickedUp { .. }));
    assert!(w.peek(held).unwrap().deleted());
    let lying = w.place(id(CAPS), cell, [0.0; 3], [0.0; 3], 5).unwrap();
    w.get(lying).unwrap().dropped_by = Some(holder);
    let snap: fnv_sim::Snapshot = serde_json::from_str(&serde_json::to_string(&w.snapshot()).unwrap()).unwrap();
    assert!(!snap.refs.contains_key(&held), "no full entry");
    assert_eq!(snap.deleted_created.get(&held), Some(&id(STIMPAK)));
    assert!(snap.refs.contains_key(&lying), "one still in the world stays");
    let mut back = World::restore(&f.forms, &f.refs, &snap).unwrap();
    assert!(back.peek(held).is_none());
    assert_eq!(back.get(held).err(), Some(SimError::Deleted(held)));
    assert_eq!(back.item_count(held, id(STIMPAK)).err(), Some(SimError::Deleted(held)), "as an inventory lookup answered before");
    assert_eq!(back.deleted_base(held), Some(id(STIMPAK)));
    assert_eq!(back.dropped_beside(holder), vec![lying], "the pruned one is skipped, not an error");
    assert_eq!(back.item_count(p, id(STIMPAK)).unwrap(), 1);
    let next = back.place(id(STIMPAK), cell, [0.0; 3], [0.0; 3], 1).unwrap();
    assert!(next.0 > lying.0, "FormIDs are not given again");
    // Saved again, the tombstone stays.
    assert_eq!(back.snapshot().deleted_created.get(&held), Some(&id(STIMPAK)));
    // An older snapshot with the whole deleted entry still loads, and the next save prunes it.
    let mut old = snap.clone();
    old.deleted_created.clear();
    old.refs.insert(held, w.peek(held).unwrap().clone());
    let mut again = World::restore(&f.forms, &f.refs, &old).unwrap();
    assert!(again.get(held).unwrap().deleted());
    assert!(!again.snapshot().refs.contains_key(&held));

    // Size after N drops and pick-ups: the whole entries the save used to keep against
    // the pruned save.
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    const N: usize = 1000;
    for _ in 0..N {
        let r = w.place(id(STIMPAK), cell, [1.0, 2.0, 3.0], [0.0; 3], 1).unwrap();
        w.activate(p, r).unwrap();
    }
    let pruned = serde_json::to_string(&w.snapshot()).unwrap().len();
    let mut whole = w.snapshot();
    whole.refs.extend(w.created().map(|(k, v)| (k, v.clone())));
    whole.deleted_created.clear();
    let whole = serde_json::to_string(&whole).unwrap().len();
    println!("snapshot after {N} drop/pick-up cycles: {whole} bytes with whole entries, {pruned} pruned");
    assert!(pruned * 4 < whole, "{pruned} vs {whole}");
}

#[test]
fn rerolled_inventory_sources_replace_and_clear_the_created_source_without_changing_placed_lifetime() {
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let made = w.place(id(SAFE), Some(id(CELL_A)), [0.0; 3], [0.0; 3], 1).unwrap();
    w.give_items_of(made, id(LOCKER)).unwrap();
    assert_eq!(w.item_count(made, id(PISTOL)).unwrap(), 1);
    w.set_inventory_source(made, Some(id(SAFE)));
    w.set_inventory_source(id(SAFE_REF), Some(id(LOCKER)));
    assert!(w.reset_inventory(made).unwrap());
    assert!(w.reset_inventory(id(SAFE_REF)).unwrap());
    assert_eq!(w.item_count(made, id(PISTOL)).unwrap(), 0);
    assert_eq!(w.item_count(id(SAFE_REF), id(PISTOL)).unwrap(), 1);
    assert_eq!(w.snapshot().created_inventory_sources, [(made, id(SAFE))].into_iter().collect());
    w.set_inventory_source(made, Some(id(LOCKER)));
    assert!(w.reset_inventory(made).unwrap());
    assert_eq!(w.item_count(made, id(PISTOL)).unwrap(), 1);
    w.set_inventory_source(made, None);
    w.set_inventory_source(id(SAFE_REF), None);
    assert!(w.snapshot().created_inventory_sources.is_empty());
    let mut back = World::restore(&f.forms, &f.refs, &w.snapshot()).unwrap();
    for reference in [made, id(SAFE_REF)] {
        assert!(back.reset_inventory(reference).unwrap());
        assert_eq!(back.item_count(reference, id(PISTOL)).unwrap(), 0);
    }
}

/// The run-time state a snapshot carries since #1084 (a created reference's inventory
/// source, the power armour flag, carry bonuses and weight rules), and what it leaves
/// to the session: game settings, leveled lists and base containers edited by script are
/// documented as not save baked but lasting for the session, so a load in the same run
/// keeps them (`World::take_session`) and a fresh run's load goes without them.
#[test]
fn snapshot_carries_run_time_state_and_leaves_session_edits() {
    use fnv_sim::inventory::WeightRules;
    use fnv_sim::settings::Setting;
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let p = FormId::PLAYER_REF;
    // Saved: a created reference's source, which no recompute gives back. Not saved: a
    // placed one's (the game layer works it out again on a load), nor its own base's.
    let made = w.place(id(SAFE), Some(id(CELL_A)), [0.0; 3], [0.0; 3], 1).unwrap();
    w.give_items_of(made, id(LOCKER)).unwrap();
    let taken = w.place(id(SAFE), Some(id(CELL_A)), [0.0; 3], [0.0; 3], 1).unwrap();
    w.give_items_of(taken, id(LOCKER)).unwrap();
    w.set_deleted(taken, true).unwrap();
    w.set_inventory_source(id(SAFE_REF), Some(id(LOCKER)));
    w.set_inventory_source(id(SAFE_REF), None);
    w.set_power_armor_training(true);
    w.set_carry_bonus(p, 25.0);
    let rules = WeightRules { light: Some((2.0, 0.5)), heavy_weapons: None, ..Default::default() };
    w.set_weight_rules(p, rules);
    // Session only.
    w.change_setting("fAVDCarryWeightsBase", Setting::Float(300.0)).unwrap();
    assert!(w.edit_leveled(id(LL_LOOT), |l| l.chance_none = 100));
    assert!(w.edit_base_entries(id(SAFE), |e| e.push(Entry::new(id(KEY), 1))));
    assert!(w.container_respawns(id(SAFE)));
    w.set_container_respawns(id(SAFE), false);
    assert!(!w.container_respawns(id(SAFE)));
    let text = serde_json::to_string(&w.snapshot()).unwrap();
    for absent in ["fAVDCarryWeightsBase", "favdcarryweightsbase", "chance_none", "container_respawns"] {
        assert!(!text.contains(absent), "{absent} is not saved");
    }
    let snap: fnv_sim::Snapshot = serde_json::from_str(&text).unwrap();
    assert_eq!(snap.created_inventory_sources, [(made, id(LOCKER))].into_iter().collect(), "neither the placed nor the deleted one");
    // A fresh run's load: the saved state is back, the session edits are not.
    let mut back = World::restore(&f.forms, &f.refs, &snap).unwrap();
    back.set_inventory_sources(Default::default());
    assert!(back.power_armor_training());
    assert_eq!(back.weight_rules(p), rules);
    assert_eq!(back.snapshot(), w.snapshot());
    assert!(back.reset_inventory(made).unwrap());
    assert_eq!(back.item_count(made, id(PISTOL)).unwrap(), 1, "LOCKER's items, the created reference's source");
    assert_eq!(back.setting_f32("fAVDCarryWeightsBase"), Some(150.0));
    assert_ne!(back.leveled_list(id(LL_LOOT)).unwrap().chance_none, 100);
    assert!(back.base_entries(id(SAFE)).unwrap().iter().all(|e| e.item != id(KEY)));
    assert!(back.container_respawns(id(SAFE)), "fresh-run restore resets session SetContainerRespawns override");
    // A load in the same run: the session edits carry over.
    back.put_session(w.take_session());
    assert_eq!(back.setting_f32("fAVDCarryWeightsBase"), Some(300.0));
    assert_eq!(back.leveled_list(id(LL_LOOT)).unwrap().chance_none, 100);
    assert!(back.base_entries(id(SAFE)).unwrap().iter().any(|e| e.item == id(KEY)));
    assert!(!back.container_respawns(id(SAFE)), "same-run put_session preserves SetContainerRespawns override");
    // A WorldSession holding only a container_respawns override is non-empty.
    let mut only_respawns = World::new(&f.forms, &f.refs, SimConfig::default());
    assert!(only_respawns.take_session().is_empty());
    only_respawns.set_container_respawns(id(SAFE), false);
    let session = only_respawns.take_session();
    assert!(!session.is_empty());
    assert!(only_respawns.take_session().is_empty());
    // Older snapshots, without the new fields, still load.
    let mut old: serde_json::Value = serde_json::from_str(&text).unwrap();
    for k in ["created_inventory_sources", "power_armor_training", "carry_bonus", "weight_rules"] {
        assert!(old.as_object_mut().unwrap().remove(k).is_some(), "{k} is written");
    }
    let old: fnv_sim::Snapshot = serde_json::from_value(old).unwrap();
    let back = World::restore(&f.forms, &f.refs, &old).unwrap();
    assert!(!back.power_armor_training());
}

/// Session-only zone overrides (`set_zone_min_level`, `set_zone_match_level`,
/// `set_cell_zone`, `set_ref_zone`) reach `enter_zone` and `level_at`, stay out of
/// `Snapshot`, reset on fresh-run restore, and survive same-run `World::put_session`
/// (#2272).
#[test]
fn session_zone_overrides_reach_entry_level_at_and_world_session() {
    let f = levels_fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    // Authored baseline: LOCK_ZONE has min_level 11, ROOM has no zone.
    assert_eq!(w.level_at(id(ZONED_CHEST), 3).level, 11);
    assert_eq!(w.level_at(id(LEVELED_CHEST), 3).level, 3);
    assert!(w.take_session().is_empty());

    // Lower LOCK_ZONE min_level to 6 and assign ROOM -> LOCK_ZONE.
    w.set_zone_min_level(id(LOCK_ZONE), 6);
    w.set_cell_zone(id(ROOM), Some(id(LOCK_ZONE)));
    assert_eq!(w.level_at(id(ZONED_CHEST), 3).level, 6);
    assert_eq!(w.level_at(id(LEVELED_CHEST), 3).level, 6);

    // Enabling Match PC Below Minimum Level makes a level-3 player stay at 3.
    w.set_zone_match_level(id(LOCK_ZONE), true);
    assert_eq!(w.level_at(id(ZONED_CHEST), 3).level, 3);

    // Not saved in Snapshot: fresh-run restore reverts to authored min_level 11 & ROOM=None.
    let snap: fnv_sim::Snapshot = serde_json::from_str(&serde_json::to_string(&w.snapshot()).unwrap()).unwrap();
    let mut fresh = World::restore(&f.forms, &f.refs, &snap).unwrap();
    assert_eq!(fresh.level_at(id(ZONED_CHEST), 3).level, 11);
    assert_eq!(fresh.level_at(id(LEVELED_CHEST), 3).level, 3);
    assert_eq!(fresh.enter_zone(id(LOCK_ZONE), 3), Some(11));

    // Same-run restore with put_session preserves all zone overrides and makes WorldSession non-empty.
    let session = w.take_session();
    assert!(!session.is_empty());
    assert!(w.take_session().is_empty());
    let mut same_run = World::restore(&f.forms, &f.refs, &snap).unwrap();
    same_run.put_session(session);
    assert_eq!(same_run.cell_zone_of(id(ROOM)), Some(id(LOCK_ZONE)));
    assert_eq!(same_run.level_at(id(LEVELED_CHEST), 3).level, 3);
    same_run.set_zone_match_level(id(LOCK_ZONE), false);
    assert_eq!(same_run.enter_zone(id(LOCK_ZONE), 3), Some(6));
    assert_eq!(same_run.level_at(id(LEVELED_CHEST), 3).level, 6);
}

// ---- a second plugin moving and deleting the master's references (#999) -----------

/// A plugin over Test.esm (master index 0) that moves STIM_REF from CELL_A to CELL_B,
/// the persistent KEY_REF from CELL_A's persistent children to CELL_B's, GUN_REF out of
/// CELL_A into the exterior cell WORLD_CELL, and marks CHAIR_REF Deleted (record flag 0x20).
fn mover_bytes() -> Vec<u8> {
    let hedr = cat(&[&1.34f32.to_le_bytes(), &0i32.to_le_bytes(), &0x800u32.to_le_bytes()]);
    let tes4 = record(b"TES4", 0, 0, &[sub(b"HEDR", hedr), sub(b"MAST", z("Test.esm")), sub(b"DATA", vec![0; 8])]);
    let cell_a = record(b"CELL", 0, CELL_A, &[sub(b"EDID", z("TestCellA"))]);
    let cell_b = record(b"CELL", 0, CELL_B, &[sub(b"EDID", z("TestCellB"))]);
    let temp_a = refr(CHAIR_REF, 0x20, CHAIR, [0.0; 3], vec![]);
    let temp_b = refr(STIM_REF, 0, STIMPAK, [40.0, 50.0, 60.0], vec![sub(b"XCNT", 3i32.to_le_bytes().to_vec())]);
    let pers_b = refr(KEY_REF, 0x400 | 0x800, KEY, [70.0, 80.0, 90.0], vec![sub(b"EDID", z("TestKeyRef"))]);
    let cells = top(
        b"CELL",
        &group(
            0i32.to_le_bytes(),
            2,
            &group(0i32.to_le_bytes(), 3, &cat(&[&cell_a, &cell_children(CELL_A, &temp_a, &[]), &cell_b, &cell_children(CELL_B, &temp_b, &pers_b)])),
        ),
    );
    let wrld = {
        let wrec = record(b"WRLD", 0, WORLD, &[sub(b"EDID", z("TestWorld"))]);
        let xclc = cat(&[&(-18i32).to_le_bytes(), &0i32.to_le_bytes()]);
        let ecell = record(b"CELL", 0, WORLD_CELL, &[sub(b"EDID", z("TestGrid")), sub(b"DATA", vec![0]), sub(b"XCLC", xclc)]);
        let gun = refr(GUN_REF, 0, PISTOL, [-73000.0, 1200.0, 8600.0], vec![]);
        let block = group(0i32.to_le_bytes(), 4, &group(0i32.to_le_bytes(), 5, &cat(&[&ecell, &cell_children(WORLD_CELL, &gun, &[])])));
        top(b"WRLD", &cat(&[&wrec, &group(WORLD.to_le_bytes(), 1, &block)]))
    };
    cat(&[&tes4, &cells, &wrld])
}

/// Each reference a later plugin moves is in its new cell only, with its new place, and
/// one it marks Deleted is in no cell (#999: per-cell child lists must not bring back a
/// moved reference in its old cell too).
#[test]
fn a_later_plugin_moves_and_deletes_references_into_one_cell_each() {
    let plugins = vec![Plugin::parse(&plugin_bytes(), "Test.esm".into()).unwrap(), Plugin::parse(&mover_bytes(), "Mover.esp".into()).unwrap()];
    let lo = LoadOrder::new(&plugins).unwrap();
    let refs = RefTable::build(&lo, &schema());
    let cells_of = |r: u32| refs.by_cell.iter().filter(|(_, v)| v.contains(&id(r))).map(|(c, v)| (*c, v.iter().filter(|x| **x == id(r)).count())).collect::<Vec<_>>();
    // Moved inside: CELL_B only, at the new place.
    assert_eq!(cells_of(STIM_REF), vec![(id(CELL_B), 1)]);
    assert_eq!((refs.get(id(STIM_REF)).unwrap().cell, refs.get(id(STIM_REF)).unwrap().pos), (Some(id(CELL_B)), [40.0, 50.0, 60.0]));
    // Moved persistent: CELL_B only.
    assert_eq!(cells_of(KEY_REF), vec![(id(CELL_B), 1)]);
    assert_eq!(refs.get(id(KEY_REF)).unwrap().pos, [70.0, 80.0, 90.0]);
    // Moved out to the exterior: WORLD_CELL only.
    assert_eq!(cells_of(GUN_REF), vec![(id(WORLD_CELL), 1)]);
    assert_eq!(refs.get(id(GUN_REF)).unwrap().cell, Some(id(WORLD_CELL)));
    // Deleted by the later plugin: in no cell.
    assert!(cells_of(CHAIR_REF).is_empty(), "{:?}", cells_of(CHAIR_REF));
    // Untouched references stay where the master put them.
    assert_eq!(cells_of(SAFE_REF), vec![(id(CELL_A), 1)]);
}

/// A reference state's saved form with its flags: fixed text, as saves hold it. Making the
/// flags private (issue #1464) kept their names, their place and that both must be there.
#[test]
fn ref_state_json_contract() {
    use fnv_sim::world::RefState;
    let text = |flags: &str| format!(r#"{{"base":"0x00000100","cell":null,"pos":[1.0,2.0,3.0],"rot":[0.0,0.0,0.5],{flags}"count":1,"open":false}}"#);
    for (enabled, deleted) in [(true, false), (false, true), (true, true), (false, false)] {
        let saved = text(&format!(r#""enabled":{enabled},"deleted":{deleted},"#));
        let state: RefState = serde_json::from_str(&saved).unwrap();
        assert_eq!((state.enabled(), state.deleted()), (enabled, deleted), "{saved}");
        assert_eq!(serde_json::to_string(&state).unwrap(), saved, "the same fields in the same order");
    }
    for (flags, missing) in [(r#""deleted":false,"#, "enabled"), (r#""enabled":true,"#, "deleted")] {
        let error = serde_json::from_str::<RefState>(&text(flags)).unwrap_err().to_string();
        assert!(error.contains(&format!("missing field `{missing}`")), "{error}");
    }
}


/// [design, #2538] Synthetic reciprocal doors: only the destination has XLOC.
#[test]
fn load_door_lock_reads_refuses_unlocks_and_restores_one_source() {
    let mut f = fixture();
    let (a, b, p) = (id(DOOR_A), id(DOOR_B), FormId::PLAYER_REF);
    f.refs.refs.get_mut(&b).unwrap().lock = Some(fnv_sim::refs::Lock { level: 255, key: None, leveled: false });
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    assert_eq!(w.lock_source(a), b);
    assert_eq!(w.lock_of(a), w.lock_of(b));
    assert_eq!(w.lock_of(a).unwrap().level, 255);
    assert_eq!(w.lock_status(a).unwrap(), 1);
    assert!(w.peek(a).is_none() && w.peek(b).is_none());
    let before = w.get(p).unwrap().clone();
    let npc = w.place(id(GUARD), before.cell, before.pos, [0.0; 3], 1).unwrap();
    for door in [a, b] {
        assert_eq!(w.activate(p, door).unwrap().outcome, Outcome::Locked { level: 255 });
        assert_eq!(w.activate_scripted(p, door).unwrap().outcome, Outcome::Locked { level: 255 });
        assert_eq!(w.activate(npc, door).unwrap().outcome, Outcome::Locked { level: 255 });
        assert_eq!((w.peek(p).unwrap().cell, w.peek(p).unwrap().pos), (before.cell, before.pos));
    }
    assert!(w.peek(a).unwrap().lock.is_none());
    let serial = w.lock_serial();
    w.unlock(a).unwrap();
    assert!(w.lock_serial() > serial);
    assert_eq!(w.lock_status(a).unwrap(), 0);
    assert_eq!(w.lock_status(b).unwrap(), 0);
    let snap: fnv_sim::Snapshot = serde_json::from_str(&serde_json::to_string(&w.snapshot()).unwrap()).unwrap();
    let mut restored = World::restore(&f.forms, &f.refs, &snap).unwrap();
    assert_eq!(restored.lock_status(a).unwrap(), 0);
    assert!(restored.peek(a).unwrap().lock.is_none());
    assert!(matches!(restored.activate(p, a).unwrap().outcome, Outcome::Teleported { .. }));
    restored.lock(a, Some(25)).unwrap();
    assert_eq!(restored.lock_source(a), a);
    assert_eq!(restored.lock_of(a).unwrap().level, 25);
    assert_eq!(restored.lock_status(b).unwrap(), 0);
    restored.lock(b, Some(100)).unwrap();
    restored.unlock(a).unwrap();
    assert_eq!(restored.lock_status(b).unwrap(), 1);
    assert_eq!(restored.lock_of(a).unwrap().level, 25);
}

#[test]
fn load_door_lock_key_empty_pairs_and_non_doors() {
    let mut f = fixture();
    let (a, b, p) = (id(DOOR_A), id(DOOR_B), FormId::PLAYER_REF);
    let w = World::new(&f.forms, &f.refs, SimConfig::default());
    assert_eq!(w.lock_of(a), None);
    assert_eq!(w.lock_source(a), a);
    drop(w);
    f.refs.refs.get_mut(&b).unwrap().lock = Some(fnv_sim::refs::Lock { level: 50, key: Some(id(KEY)), leveled: false });
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    assert_eq!(w.lock_of(a).unwrap().key, Some(id(KEY)));
    w.add_item(p, id(KEY), 1).unwrap();
    let result = w.activate(p, a).unwrap();
    assert_eq!(result.unlocked_with, Some(id(KEY)));
    assert!(matches!(result.outcome, Outcome::Teleported { .. }));
    assert_eq!(w.lock_status(b).unwrap(), 0);
    assert_eq!(w.item_count(p, id(KEY)).unwrap(), 1);
    drop(w);
    f.refs.refs.get_mut(&a).unwrap().base = id(SAFE);
    let w = World::new(&f.forms, &f.refs, SimConfig::default());
    assert_eq!(w.lock_of(a), None, "XTEL on a non-door does not inherit a lock");
    drop(w);
    f.refs.refs.get_mut(&a).unwrap().base = id(DOOR);
    f.refs.refs.remove(&b);
    let w = World::new(&f.forms, &f.refs, SimConfig::default());
    assert_eq!(w.lock_of(a), None, "missing partners are not locks");
}

/// A created actor keeps where it was made across a save (`RefState::made_at`, its
/// editor location, #2278), through the save's JSON; a created actor of a save written
/// before that field existed takes the place it was saved at, and a dropped item, which
/// has no creation place by design, still has none (#3042).
#[test]
fn a_created_actor_keeps_where_it_was_made_across_a_save() {
    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());
    let made_here = [100.0, 200.0, 0.0];
    let walked_to = [900.0, -300.0, 10.0];
    let g = w.place(id(GUARD), Some(id(CELL_A)), made_here, [0.0; 3], 1).unwrap();
    w.get(g).unwrap().pos = walked_to;
    w.add_item(FormId::PLAYER_REF, id(STIMPAK), 1).unwrap();
    let item = w.drop_item(FormId::PLAYER_REF, id(STIMPAK), 1).unwrap()[0];
    assert_eq!(w.peek(item).unwrap().made_at, None);
    let json = serde_json::to_value(w.snapshot()).unwrap();
    let back = World::restore(&f.forms, &f.refs, &serde_json::from_value(json.clone()).unwrap()).unwrap();
    assert_eq!(back.peek(g).unwrap().made_at, Some((Some(id(CELL_A)), made_here)));
    assert_eq!(back.peek(g).unwrap().pos, walked_to);
    assert_eq!(back.peek(item).unwrap().made_at, None, "a dropped item has no creation place");
    // A save from before the field: the actor's entry has no `made_at`.
    let mut old = json;
    let entry = old["refs"].as_object_mut().unwrap().get_mut(&format!("{g}")).expect("the actor's entry by its FormID");
    assert!(entry.as_object_mut().unwrap().remove("made_at").is_some());
    let back = World::restore(&f.forms, &f.refs, &serde_json::from_value(old).unwrap()).unwrap();
    assert_eq!(back.peek(g).unwrap().made_at, Some((Some(id(CELL_A)), walked_to)), "the place it was saved at");
    assert_eq!(back.peek(item).unwrap().made_at, None);
    // And the next save keeps it.
    let again = World::restore(&f.forms, &f.refs, &back.snapshot()).unwrap();
    assert_eq!(again.peek(g).unwrap().made_at, Some((Some(id(CELL_A)), walked_to)));
}

#[test]
fn builtin_default_gmsts_resolve_mutate_and_yield_to_esm_overrides() {
    use fnv_sim::settings::Setting;

    let f = fixture();
    let mut w = World::new(&f.forms, &f.refs, SimConfig::default());

    // Absent from the synthetic ESM, all 7 seeded engine defaults resolve and mutate.
    assert_eq!(w.setting_value("fCombatEnvironmentBloodChance"), Some(&Setting::Float(0.75)));
    assert_eq!(w.setting_value("iDismemberBloodDecalCount"), Some(&Setting::Int(10)));
    assert_eq!(w.setting_value("fGunWobbleChaseDriftTime"), Some(&Setting::Float(0.75)));
    assert_eq!(w.setting_value("fIronSightsFOVTimeChange"), Some(&Setting::Float(0.25)));
    assert_eq!(w.setting_value("fKnockbackForceMax"), None);
    assert_eq!(w.setting_value("FCharacterDefaultTurningSpeed"), Some(&Setting::Float(90.0)));
    assert_eq!(w.setting_value("fcharacterdefaultturningspeed"), Some(&Setting::Float(90.0)));
    assert_eq!(w.setting_value("iAntiCheatDuration"), Some(&Setting::Int(60)));
    assert_eq!(w.setting_value("fSeenDataUpdateRadius"), Some(&Setting::Float(1024.0)));

    // Session mutation works case-insensitively and enforces setting kind.
    w.change_setting("fCombatEnvironmentBloodChance", Setting::Float(0.35)).unwrap();
    assert_eq!(w.setting_f32("FCOMBATENVIRONMENTBLOODCHANCE"), Some(0.35));
    w.change_setting("fcharacterdefaultturningspeed", Setting::Float(180.0)).unwrap();
    assert_eq!(w.setting_f32("FCharacterDefaultTurningSpeed"), Some(180.0));
    w.change_setting("iAntiCheatDuration", Setting::Int(0)).unwrap();
    assert_eq!(w.setting_value("iAntiCheatDuration"), Some(&Setting::Int(0)));

    assert!(matches!(
        w.change_setting("iAntiCheatDuration", Setting::Float(1.5)),
        Err(SimError::SettingKind(_))
    ));
    assert!(matches!(
        w.change_setting("fCombatEnvironmentBloodChance", Setting::Int(1)),
        Err(SimError::SettingKind(_))
    ));
    assert!(matches!(
        w.change_setting("fNotABuiltinOrEsmSetting", Setting::Float(1.0)),
        Err(SimError::MissingSetting(_))
    ));

    // Session edits on seeded defaults stay session-only (not saved in Snapshot, carried by put_session).
    let snap = w.snapshot();
    let mut restored = World::restore(&f.forms, &f.refs, &snap).unwrap();
    assert_eq!(restored.setting_f32("fCombatEnvironmentBloodChance"), Some(0.75));
    restored.put_session(w.take_session());
    assert_eq!(restored.setting_f32("fCombatEnvironmentBloodChance"), Some(0.35));

    // An ESM GMST record (including uppercase-prefixed `FCharacterDefaultTurningSpeed`)
    // always overrides the seeded engine default.
    let gmsts = [
        record(b"GMST", 0, 0x901, &[sub(b"EDID", z("fCombatEnvironmentBloodChance")), sub(b"DATA", 0.25f32.to_le_bytes().to_vec())]),
        record(b"GMST", 0, 0x902, &[sub(b"EDID", z("FCharacterDefaultTurningSpeed")), sub(b"DATA", 135.0f32.to_le_bytes().to_vec())]),
    ]
    .concat();
    let bytes = [tes4(), top(b"GMST", &gmsts)].concat();
    let plugins = vec![Plugin::parse(&bytes, "gmst-override.esm".into()).unwrap()];
    let lo = LoadOrder::new(&plugins).unwrap();
    let s = schema();
    let forms = FormStore::build(&lo, &s);
    let refs = RefTable::build(&lo, &s);
    assert!(forms.issues.is_empty(), "uppercase GMST prefix should decode without issues: {:?}", forms.issues.to_json());
    let mut w_esm = World::new(&forms, &refs, SimConfig::default());
    assert_eq!(w_esm.setting_value("fCombatEnvironmentBloodChance"), Some(&Setting::Float(0.25)));
    assert_eq!(w_esm.setting_value("FCharacterDefaultTurningSpeed"), Some(&Setting::Float(135.0)));
    w_esm.change_setting("FCharacterDefaultTurningSpeed", Setting::Float(225.0)).unwrap();
    assert_eq!(w_esm.setting_f32("fCharacterDefaultTurningSpeed"), Some(225.0));
}
