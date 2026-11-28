//! Step C2 rules (docs/spec-notes/combat-rules.md): damage, spread, the attack cycle and
//! projectiles, on hand-made stats and a toy world. The expected numbers follow from the
//! documented formulas and our CombatTuning defaults.

use fnv_combat::attack::{ClipTiming, Cycle, Event, Input, Phase, RoundReload, Timing};
use fnv_combat::damage::{self, Attacker, HitInput, HitPart, NoPerks, Target};
use fnv_combat::data::{AmmoEffect, ExplosionStats, ProjectileStats, WeaponStats};
use fnv_combat::melee::NextAttack;
use fnv_combat::projectile::{blast, swing, swing_reaching, ActorId, CombatWorld, ProjEvent, Projectile, WorldHit};
use fnv_combat::shot::{self, Seen};
use fnv_combat::spread::{self, Kick, Stance};
use fnv_combat::vec::{self, V3};
use fnv_combat::{CombatTuning, Rules};
use fnv_sim::FormId;

fn gun(base: i32) -> WeaponStats {
    WeaponStats {
        base_damage: base,
        anim_type: Some("Pistol - Ballistic (1 Hand)".into()),
        crit_damage: 10,
        crit_mult: 1.0,
        clip_size: 2,
        ammo_use: 1,
        attack_mult: 1.0,
        anim_mult: 1.0,
        attack_shots_per_sec: 2.0,
        min_spread: 1.0,
        ..WeaponStats::default()
    }
}

fn effect(kind: &str, op: &str, value: f32) -> AmmoEffect {
    AmmoEffect { kind: Some(kind.into()), operation: Some(op.into()), value, ..AmmoEffect::default() }
}

fn skilled() -> Attacker {
    Attacker { skill: 100.0, ..Attacker::default() }
}

/// One hit with no perks and a crit roll that never crits unless forced.
fn hit(w: &WeaponStats, projectiles: u32, a: &Attacker, t: &Target, effects: &[&AmmoEffect], part: HitPart, roll: f32) -> damage::Damage {
    let h = HitInput { weapon: w, projectiles, condition: 1.0, ammo_effects: effects, part, power: 1.0 };
    damage::hit(&Rules::defaults(), &CombatTuning::default(), &h, a, t, &NoPerks, &NoPerks, roll)
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
}

#[test]
fn gun_damage_skill_and_dt() {
    let w = gun(20);
    let d = hit(&w, 1, &skilled(), &Target { dt: 5.0, ..Target::default() }, &[], HitPart::default(), 0.99);
    assert!(close(d.base, 20.0) && close(d.dealt, 15.0) && !d.critical, "{d:?}");
    // Skill 0: half damage [documented skill bonus 0.5 + 0.5 × skill / 100].
    let d = hit(&w, 1, &Attacker { skill: 0.0, ..Attacker::default() }, &Target::default(), &[], HitPart::default(), 0.99);
    assert!(close(d.dealt, 10.0));
    // DT never takes more than 80 % (dt_floor 0.2).
    let d = hit(&w, 1, &skilled(), &Target { dt: 30.0, ..Target::default() }, &[], HitPart::default(), 0.99);
    assert!(close(d.dealt, 4.0));
    // DR scales what DT leaves.
    let d = hit(&w, 1, &skilled(), &Target { dt: 5.0, dr: 50.0, ..Target::default() }, &[], HitPart::default(), 0.99);
    assert!(close(d.dealt, 7.5));
}

#[test]
fn a_damage_type_takes_its_resistance_as_well_as_dr() {
    // DR cuts a laser's hit [observed, damage-armour.md §4] and Energy Resistance does
    // too (see docs/spec-notes/weapon-data-fields.md §3); DT first.
    let laser = WeaponStats { resist: Some("Energy Resistance".into()), ..gun(20) };
    assert_eq!(damage::resistance_of(&laser), Some("Energy Resistance"));
    let d = hit(&laser, 1, &skilled(), &Target { dt: 5.0, dr: 50.0, resist: 20.0, ..Target::default() }, &[], HitPart::default(), 0.99);
    assert!(close(d.dr, 50.0) && close(d.dealt, 6.0), "{d:?}");
    // No resistance of that kind: DR alone.
    let d = hit(&laser, 1, &skilled(), &Target { dt: 5.0, dr: 50.0, ..Target::default() }, &[], HitPart::default(), 0.99);
    assert!(close(d.dealt, 7.5));
    // Capped as DR is (fMaxArmorRating 85), and never under the floor's 20 %.
    let d = hit(&laser, 1, &skilled(), &Target { resist: 100.0, ..Target::default() }, &[], HitPart::default(), 0.99);
    assert!(close(d.dr, 0.0) && close(d.dealt, 4.0), "{d:?}");
    // None and Damage Resistance keep DR.
    for r in ["None", "Damage Resistance"] {
        let w = WeaponStats { resist: Some(r.into()), ..gun(20) };
        assert_eq!(damage::resistance_of(&w), None);
        assert!(close(hit(&w, 1, &skilled(), &Target { dt: 5.0, dr: 50.0, resist: 90.0, ..Target::default() }, &[], HitPart::default(), 0.99).dealt, 7.5));
    }
}

/// Hits between NPCs in the original's NCR-Fiend fight (npc-behaviour-compare.md §9,
/// combat-rules.md §18), with the records' values from FalloutNV.esm: a trooper (Guns
/// 46) with a Service Rifle (18, crit 18) at 50 % on Cook-Cook (Metal Armor DT 12 +
/// Raider Arclight Helmet DT 1) did exactly 2.1875, the 20 % floor of 10.9375. The same
/// rule gives 5.9 on a Fiend (DT 5) and 15.9 for a critical on Cook-Cook. A Fiend (Guns
/// 63) with a Hunting Rifle (52) at 45 % on a trooper (NCR Face Wrap Armor DT 10 at 35 %
/// and Trooper Helmet DT 2) does 23.4.
#[test]
fn npc_against_npc_hits_of_the_originals_fight() {
    let rifle = |base: i32, crit: u32| WeaponStats { base_damage: base, crit_damage: crit, anim_type: Some("Rifle - Automatic (2 Hand)".into()), ..gun(base) };
    let shot = |w: &WeaponStats, skill: f32, condition: f32, dt: f32, roll: f32| {
        let h = HitInput { weapon: w, projectiles: 1, condition, ammo_effects: &[], part: HitPart::default(), power: 1.0 };
        let a = Attacker { skill, ..Attacker::default() };
        damage::hit(&Rules::defaults(), &CombatTuning::default(), &h, &a, &Target { dt, ..Target::default() }, &NoPerks, &NoPerks, roll)
    };
    let service = rifle(18, 18);
    let d = shot(&service, 46.0, 0.5, 13.0, 0.99);
    // The original's 10.9375 and 2.1875 to within 0.002 (its condition penalty was
    // measured to three places).
    let near = |a: f32, b: f32| (a - b).abs() < 0.002;
    assert!(near(d.raw, 10.9375) && near(d.dealt, 2.1875), "{d:?}");
    assert!(near(shot(&service, 46.0, 0.5, 5.0, 0.99).dealt, 5.9375));
    let crit = shot(&service, 46.0, 0.5, 13.0, 0.0);
    assert!(crit.critical && near(crit.dealt, 15.9375), "{crit:?}");
    let trooper_dt = 10.0 * fnv_combat::condition::armor_mult(0.35) + 2.0;
    let d = shot(&rifle(52, 52), 63.0, 0.45, trooper_dt, 0.99);
    assert!((d.dealt - 23.36).abs() < 0.01, "{d:?}");
}

/// The original's figures, hit by hit, at skill 50 with a weapon in full condition
/// [observed 2026-11-21: the damage parity matrix, docs/spec-notes/damage-armour.md].
#[test]
fn armour_steps_match_the_measured_hits() {
    let a = Attacker::default();
    let near = |d: &damage::Damage, v: f32| (d.dealt - v).abs() < 0.01;
    let (torso, head) = (HitPart::default(), HitPart { damage_mult: 2.0, head: true });
    let on = |dt: f32, dr: f32| Target { dt, dr, ..Target::default() };
    let rifle = WeaponStats { crit_damage: 52, ..gun(52) };
    let pistol = WeaponStats { crit_damage: 16, ..gun(16) };
    // The part's multiplier after DT, the floor and DR (#3215).
    assert!(near(&hit(&pistol, 1, &a, &on(4.0, 0.0), &[], head.clone(), 0.99), 16.0));
    assert!(near(&hit(&pistol, 1, &a, &on(11.0, 0.0), &[], torso.clone(), 0.99), 2.4));
    assert!(near(&hit(&pistol, 1, &a, &on(11.0, 0.0), &[], head.clone(), 0.99), 4.8));
    assert!(near(&hit(&rifle, 1, &a, &on(19.0, 30.0), &[], torso.clone(), 0.99), 14.0));
    assert!(near(&hit(&rifle, 1, &a, &on(19.0, 30.0), &[], head.clone(), 0.99), 28.0));
    // A critical: its bonus before DT, doubled on the head with the rest.
    assert!(near(&hit(&pistol, 1, &a, &on(11.0, 0.0), &[], head.clone(), 0.0), 34.0));
    assert!(near(&hit(&rifle, 1, &a, &on(11.0, 0.0), &[], head.clone(), 0.0), 160.0));
    // DR does not cut the floor (#3216).
    assert!(near(&hit(&pistol, 1, &a, &on(21.0, 30.0), &[], torso.clone(), 0.99), 2.4));
    // Hollow Point (damage x1.75, DT x3): the floor is of the 39 before the ammunition.
    let hp = [effect("Damage Mod", "Multiply", 1.75), effect("DT Mod", "Multiply", 3.0)];
    let hp: Vec<&AmmoEffect> = hp.iter().collect();
    assert!(near(&hit(&rifle, 1, &a, &on(2.0, 0.0), &hp, torso.clone(), 0.99), 62.25));
    assert!(near(&hit(&rifle, 1, &a, &on(25.0, 0.0), &hp, torso.clone(), 0.99), 7.8));
    assert!(near(&hit(&rifle, 1, &a, &on(24.0, 30.0), &hp, head.clone(), 0.99), 15.6));
    // The ammunition's multiplier is on the critical bonus too (#3217).
    assert!(near(&hit(&rifle, 1, &a, &on(0.0, 0.0), &hp, torso.clone(), 0.0), 159.25));
    assert!(near(&hit(&rifle, 1, &a, &on(24.0, 30.0), &hp, torso.clone(), 0.0), 61.075));
    let ap = [effect("Damage Mod", "Multiply", 0.95), effect("DT Mod", "Subtract", 15.0)];
    let ap: Vec<&AmmoEffect> = ap.iter().collect();
    assert!(near(&hit(&rifle, 1, &a, &on(11.0, 0.0), &ap, torso.clone(), 0.0), 86.45));
    assert!(near(&hit(&rifle, 1, &a, &on(19.0, 30.0), &ap, torso.clone(), 0.99), 23.135));
    // Buckshot (x1.2), 7 pellets of a 70 shotgun: the floor from 7.5, and a pellet's
    // critical adds the weapon's whole 10.
    let shotgun = WeaponStats { crit_damage: 10, ..gun(70) };
    let buck = [effect("Damage Mod", "Multiply", 1.2)];
    let buck: Vec<&AmmoEffect> = buck.iter().collect();
    assert!(near(&hit(&shotgun, 7, &a, &on(0.0, 0.0), &buck, torso.clone(), 0.99), 9.0));
    assert!(near(&hit(&shotgun, 7, &a, &on(11.0, 0.0), &buck, torso.clone(), 0.99), 1.5));
    assert!(near(&hit(&shotgun, 7, &a, &on(5.0, 0.0), &buck, torso.clone(), 0.0), 16.0));
    assert!(near(&hit(&shotgun, 7, &a, &on(18.0, 0.0), &buck, torso.clone(), 0.0), 3.5));
    // DT leaves 4, DR would make it 2.8: the floor's 3.5 holds after DR too.
    assert!(near(&hit(&shotgun, 7, &a, &on(17.0, 30.0), &buck, torso.clone(), 0.0), 3.5));
    assert!(near(&hit(&shotgun, 7, &a, &on(25.0, 0.0), &buck, head.clone(), 0.0), 7.0));
    // DR cuts a laser and a plasma rifle (the cell takes 2 off DT) (#3218).
    let cell = [effect("DT Mod", "Subtract", 2.0)];
    let cell: Vec<&AmmoEffect> = cell.iter().collect();
    let laser = WeaponStats { resist: Some("Energy Resistance".into()), ..gun(22) };
    let plasma = WeaponStats { resist: Some("Energy Resistance".into()), ..gun(47) };
    assert!(near(&hit(&laser, 1, &a, &on(0.0, 30.0), &cell, torso.clone(), 0.99), 11.55));
    assert!(near(&hit(&plasma, 1, &a, &on(24.0, 30.0), &cell, torso.clone(), 0.99), 9.275));
    assert!(near(&hit(&laser, 1, &a, &on(21.0, 30.0), &cell, head, 0.99), 6.6));
}

#[test]
fn shotgun_damage_is_shared() {
    let d = hit(&gun(100), 14, &skilled(), &Target::default(), &[], HitPart::default(), 0.99);
    assert!(close(d.dealt, 100.0 / 14.0));
}

#[test]
fn criticals() {
    let w = gun(20);
    // Roll under Luck 5 % → critical, its damage added before DT.
    let d = hit(&w, 1, &skilled(), &Target { dt: 5.0, ..Target::default() }, &[], HitPart::default(), 0.0);
    assert!(d.critical && close(d.dealt, 25.0), "{d:?}");
    // Automatic: the weapon multiplier is divided by the fire rate.
    let auto = WeaponStats { automatic: true, fire_rate: 8.0, ..gun(20) };
    let c = damage::crit_chance(&Rules::defaults(), &auto, &Attacker { luck: 10.0, ..Attacker::default() }, &NoPerks, &NoPerks);
    assert!(close(c, 1.25));
    // A headshot on an unaware target always crits [design].
    let d = hit(&w, 1, &skilled(), &Target { unaware: true, ..Target::default() }, &[], HitPart { damage_mult: 2.0, head: true }, 0.99);
    assert!(d.critical && close(d.dealt, 60.0), "the part's x2 is on the critical bonus too: {d:?}");
    // No critical without a crit multiplier.
    let none = WeaponStats { crit_mult: 0.0, ..gun(20) };
    assert!(!hit(&none, 1, &skilled(), &Target { unaware: true, ..Target::default() }, &[], HitPart { damage_mult: 1.0, head: true }, 0.0).critical);
}

#[test]
fn limb_damage_leaves_out_the_part_multiplier() {
    // The original's limb loss over its health damage is Limb Dmg Mult × 100 ÷ (max
    // health × health % × the part's damage mult) [observed 2026-10-25, 3,254 hits]: the
    // limb takes the hit without the part's multiplier.
    let w = gun(20);
    let body = hit(&w, 1, &skilled(), &Target::default(), &[], HitPart::default(), 0.99);
    assert!(close(body.dealt, 20.0) && close(body.limb_damage(1.0), 20.0), "{body:?}");
    let head = hit(&w, 1, &skilled(), &Target::default(), &[], HitPart { damage_mult: 2.0, head: true }, 0.99);
    assert!(close(head.dealt, 40.0), "{head:?}");
    assert!(close(head.limb_damage(1.0), 20.0), "the head's ×2 is not in the limb damage");
    assert!(close(head.limb_damage(2.0), 40.0), "the weapon's Limb Dmg Mult is (a knife's 2)");
    assert!(close(head.limb_damage(0.0), 0.0) && close(head.limb_damage(-1.0), 0.0));
    // Armour first, then the part multiplier: DT 10 off the hit, then the head's ×2.
    let armoured = hit(&w, 1, &skilled(), &Target { dt: 10.0, ..Target::default() }, &[], HitPart { damage_mult: 2.0, head: true }, 0.99);
    assert!(close(armoured.dealt, 20.0) && close(armoured.limb_damage(1.0), 10.0), "{armoured:?}");
    // A part of multiplier 0 deals no health damage and so no limb damage.
    let none = hit(&w, 1, &skilled(), &Target::default(), &[], HitPart { damage_mult: 0.0, head: false }, 0.99);
    assert!(close(none.limb_damage(1.0), 0.0));
}

#[test]
fn sneak_melee_unarmed() {
    // A sneak attack: × 2 for a gun, and always a critical (+ Crit Dmg 10) [owner
    // statement 2026-10-25], but never with a crit multiplier of 0 [documented].
    let sneak = Attacker { sneaking: true, ..skilled() };
    let d = hit(&gun(20), 1, &sneak, &Target::default(), &[], HitPart::default(), 0.99);
    assert!(d.critical && close(d.dealt, 50.0), "{d:?}");
    let no_crit = WeaponStats { crit_mult: 0.0, ..gun(20) };
    let d = hit(&no_crit, 1, &sneak, &Target::default(), &[], HitPart::default(), 0.99);
    assert!(!d.critical && close(d.dealt, 40.0), "{d:?}");
    let blade = WeaponStats { anim_type: Some("Melee (1 Hand)".into()), ..gun(10) };
    // 10 × skill 1.0 + MeleeDamage 0.5 × Strength 10.
    let strong = Attacker { strength: 10.0, ..skilled() };
    assert!(close(hit(&blade, 1, &strong, &Target::default(), &[], HitPart::default(), 0.99).dealt, 15.0));
    // Crippled arm: 10 × (0.2 − 1 + 0) / 2 = −4.
    let crippled = Attacker { arm_condition: 0.0, ..strong.clone() };
    assert!(close(hit(&blade, 1, &crippled, &Target::default(), &[], HitPart::default(), 0.99).dealt, 11.0));
    // Melee sneak ×5, a critical.
    let sneaky = Attacker { sneaking: true, ..strong };
    assert!(close(hit(&blade, 1, &sneaky, &Target::default(), &[], HitPart::default(), 0.99).dealt, 85.0));
    // Unarmed: 2 + UnarmedDamage (0.5 + 0.05 × 100), no skill bonus.
    let fists = WeaponStats { anim_type: Some("Hand to Hand".into()), ..gun(2) };
    let boxer = Attacker { unarmed_skill: 100.0, ..Attacker::default() };
    assert!(close(hit(&fists, 1, &boxer, &Target::default(), &[], HitPart::default(), 0.99).dealt, 7.5));
}

#[test]
fn ammo_effects_child_miss_armour_flag() {
    let w = gun(20);
    let hp = effect("Damage Mod", "Multiply", 1.2);
    let ap = effect("DT Mod", "Subtract", 2.0);
    let d = hit(&w, 1, &skilled(), &Target { dt: 5.0, ..Target::default() }, &[&hp, &ap], HitPart::default(), 0.99);
    assert!(close(d.dt, 3.0) && close(d.dealt, 21.0), "{d:?}");
    assert_eq!(hit(&w, 1, &skilled(), &Target { child: true, ..Target::default() }, &[], HitPart::default(), 0.0).dealt, 0.0);
    let zero = hit(&gun(0), 1, &skilled(), &Target::default(), &[], HitPart::default(), 0.0);
    assert!(zero.miss && !zero.critical && zero.dealt == 0.0);
    let piercing = WeaponStats { flags: vec!["Ignores Normal Weapon Resistance".into()], ..gun(20) };
    assert!(close(hit(&piercing, 1, &skilled(), &Target { dt: 15.0, ..Target::default() }, &[], HitPart::default(), 0.99).dealt, 20.0));
    // Full damage at and above 75 % condition, then 1 − (0.75 − condition) × 0.67: 0.4975
    // at zero (GECK wiki SetConditionDamagePenalty).
    let at = |condition: f32| {
        let h = HitInput { weapon: &w, projectiles: 1, condition, ammo_effects: &[], part: HitPart::default(), power: 1.0 };
        damage::hit(&Rules::defaults(), &CombatTuning::default(), &h, &skilled(), &Target::default(), &NoPerks, &NoPerks, 0.99).dealt
    };
    assert!(close(at(1.0), 20.0) && close(at(0.75), 20.0) && close(at(0.0), 9.95), "{} {}", at(0.75), at(0.0));
    // The troopers' Service Rifles of the original (npc-behaviour-compare.md §9): at 50, 45
    // and 30 % they dealt 0.832, 0.799 and 0.698 of the full-condition hit.
    for (c, seen) in [(0.5, 0.832), (0.45, 0.799), (0.3, 0.698)] {
        assert!((damage::condition_mult(c) - seen).abs() < 0.002, "{c}: {}", damage::condition_mult(c));
    }
}

#[test]
fn knockdown_knockback_explosion_falloff() {
    let r = Rules::defaults();
    assert_eq!(damage::knockdown_chance(&r, 20.0, 5.0, 60.0, 100.0), 0.0);
    assert_eq!(damage::knockdown_chance(&r, 80.0, 5.0, 100.0, 100.0), 1.0);
    assert!(close(damage::knockdown_chance(&r, 40.0, 5.0, 60.0, 100.0), 0.24));
    assert!(close(damage::knockback_force(&r, 10.0, 5.0), 90.0));
    let t = CombatTuning::default();
    assert!(close(damage::explosion_damage(&t, 100.0, 400.0, 0.0), 100.0));
    assert!(close(damage::explosion_damage(&t, 100.0, 400.0, 200.0), 62.5));
    assert!(close(damage::explosion_damage(&t, 100.0, 400.0, 400.0), 25.0));
    assert_eq!(damage::explosion_damage(&t, 100.0, 400.0, 401.0), 0.0);
}

fn ops(point: u8, op: damage::PerkOp) -> damage::EntryOps {
    damage::EntryOps(vec![(point, op, "Perk".into())])
}

#[test]
fn a_blast_passes_the_damage_and_dt_entry_points() {
    use damage::entry::{CALCULATE_WEAPON_DAMAGE, MODIFY_DAMAGE_THRESHOLD_ATTACKER, MODIFY_DAMAGE_THRESHOLD_DEFENDER};
    use damage::PerkOp::{Add, Mul, Set};
    let (r, t) = (Rules::defaults(), CombatTuning::default());
    let blast = |dt, dr, a: &dyn damage::PerkHooks, d: &dyn damage::PerkHooks| damage::blast_hit(&r, &t, 100.0, dt, dr, a, d);
    let demolition = ops(CALCULATE_WEAPON_DAMAGE, Mul(1.2));
    let deck = ops(MODIFY_DAMAGE_THRESHOLD_DEFENDER, Add(25.0));
    // Without perks: DT off the damage, then DR.
    let plain = blast(10.0, 20.0, &NoPerks, &NoPerks);
    assert!(close(plain.modified, 100.0) && close(plain.dt, 10.0) && close(plain.dealt, 72.0), "{plain:?}");
    assert!(close(blast(0.0, 0.0, &NoPerks, &NoPerks).dealt, 100.0));
    // The damage perk comes before the defender's DT: 100 x 1.2 - 25.
    let both = blast(0.0, 0.0, &demolition, &deck);
    assert!(close(both.base, 100.0) && close(both.modified, 120.0) && close(both.dt, 25.0) && close(both.dealt, 95.0), "{both:?}");
    // The attacker's entry takes DT off, never below 0.
    let surgeon = ops(MODIFY_DAMAGE_THRESHOLD_ATTACKER, Set(10.0));
    assert!(close(blast(25.0, 0.0, &surgeon, &NoPerks).dealt, 85.0));
    assert!(close(blast(4.0, 0.0, &surgeon, &NoPerks).dt, 0.0));
    // The DT floor holds with a perk's DT; DR scales what is left, under its cap.
    let floor = blast(0.0, 0.0, &NoPerks, &ops(MODIFY_DAMAGE_THRESHOLD_DEFENDER, Add(500.0)));
    assert!(close(floor.dealt, 100.0 * t.dt_floor), "{floor:?}");
    let capped = blast(0.0, 500.0, &NoPerks, &NoPerks);
    assert!(close(capped.dr, r.max_armor_rating) && close(capped.dealt, 100.0 * (1.0 - r.max_armor_rating / 100.0)), "{capped:?}");
}

#[test]
fn hit_reactions_on_crippling_and_crippled_parts() {
    let t = CombatTuning { stagger_cooldown: 3.0, ..CombatTuning::default() };
    // A hit on a healthy part does nothing, however long ago the last reaction was.
    assert!(!damage::staggers(&t, false, false, false, None));
    assert!(!damage::staggers(&t, false, false, false, Some(100.0)));
    // The crippling hit always reacts, also right after another reaction.
    assert!(damage::staggers(&t, true, true, false, None));
    assert!(damage::staggers(&t, true, true, false, Some(-0.5)));
    // Later hits on the crippled part: not while the last reaction plays, and only
    // once the cooldown after it has run out.
    assert!(damage::staggers(&t, false, true, false, None));
    assert!(!damage::staggers(&t, false, true, false, Some(-0.5)));
    assert!(!damage::staggers(&t, false, true, false, Some(0.1)));
    assert!(!damage::staggers(&t, false, true, false, Some(2.9)));
    assert!(damage::staggers(&t, false, true, false, Some(3.0)));
    // A crippled torso: any hit reacts, after the cooldown.
    assert!(damage::staggers(&t, false, false, true, None));
    assert!(!damage::staggers(&t, false, false, true, Some(1.0)));
    assert!(damage::staggers(&t, false, false, true, Some(3.0)));
}

#[test]
fn crippled_arms_in_melee_by_weapon() {
    let blade = WeaponStats { anim_type: Some("Melee (1 Hand)".into()), ..gun(10) };
    let hammer = WeaponStats { anim_type: Some("Melee (2 Hand)".into()), ..gun(10) };
    let fists = WeaponStats { anim_type: Some("Hand to Hand".into()), ..gun(2) };
    // One-handed: the right arm only.
    assert_eq!(damage::arm_condition(&blade, false, false), 1.0);
    assert_eq!(damage::arm_condition(&blade, true, false), 1.0);
    assert_eq!(damage::arm_condition(&blade, false, true), 0.0);
    // Two-handed and unarmed: half for each crippled arm.
    assert_eq!(damage::arm_condition(&hammer, true, false), 0.5);
    assert_eq!(damage::arm_condition(&hammer, true, true), 0.0);
    assert_eq!(damage::arm_condition(&fists, false, true), 0.5);
}

#[test]
fn crippled_head_and_arms_widen_the_spread() {
    let (r, t) = (Rules::defaults(), CombatTuning::default());
    let rifle = WeaponStats { anim_type: Some("2 Hand Rifle".into()), ..gun(10) };
    let calm = Stance { aiming: true, sneaking: true, skill: 100.0, strength: 10.0, aiming_move_divisor: 1.0, ..Stance::default() };
    assert_eq!(spread::crippled_spread(&r, &t, &rifle, &calm), 0.0);
    let head = Stance { head_crippled: true, ..calm.clone() };
    assert!(close(spread::crippled_spread(&r, &t, &rifle, &head), t.spread_crippled_head));
    let arm = Stance { left_arm_crippled: true, ..calm.clone() };
    assert!(close(spread::crippled_spread(&r, &t, &rifle, &arm), r.spread_crippled_arm_2h));
    assert!(spread::actor_spread(&r, &t, &rifle, &head) > spread::actor_spread(&r, &t, &rifle, &calm));
}

#[test]
fn spread_perks_cut_the_strength_and_moving_penalties() {
    let (r, t) = (Rules::defaults(), CombatTuning::default());
    assert!(r.strength_req_penalty > 0.0);
    // A weapon wanting Strength 8 in the hands of Strength 5: three points short.
    let heavy = WeaponStats { strength_req: 8, ..gun(20) };
    let weak = Stance { skill: 0.0, strength: 5.0, aiming_move_divisor: 1.0, ..Stance::default() };
    let plain = spread::actor_spread(&r, &t, &heavy, &weak);
    // Weapon Handling takes 2 off the requirement: one point short.
    let handled = Stance { strength_req_cut: 2.0, ..weak.clone() };
    assert!(close(plain - spread::actor_spread(&r, &t, &heavy, &handled), 2.0 * r.strength_req_penalty));
    // Never below no shortfall.
    let more = Stance { strength_req_cut: 9.0, ..weak.clone() };
    assert!(close(plain - spread::actor_spread(&r, &t, &heavy, &more), 3.0 * r.strength_req_penalty));
    // Run 'n Gun halves the running penalty; a divisor under 1 counts as 1.
    let w = gun(20);
    let hip = Stance { skill: 0.0, strength: 10.0, aiming_move_divisor: 1.0, ..Stance::default() };
    let running = Stance { moving: true, running: true, ..hip.clone() };
    let base = spread::actor_spread(&r, &t, &w, &hip);
    let run = spread::actor_spread(&r, &t, &w, &running);
    let gunning = Stance { aiming_move_divisor: 2.0, ..running.clone() };
    assert!(close(spread::actor_spread(&r, &t, &w, &gunning) - base, (run - base) / 2.0));
    let slow = Stance { aiming_move_divisor: 0.5, ..running.clone() };
    assert!(close(spread::actor_spread(&r, &t, &w, &slow), run));
}

#[test]
fn spread_formula_cone_bloom_recoil() {
    let r = Rules::defaults();
    let t = CombatTuning::default();
    let w = gun(20);
    let best = Stance { aiming: true, sneaking: true, skill: 100.0, strength: 10.0, aiming_move_divisor: 1.0, ..Stance::default() };
    assert!(close(spread::actor_spread(&r, &t, &w, &best), r.min_gun_spread));
    let hip = Stance { skill: 0.0, strength: 10.0, aiming_move_divisor: 1.0, ..Stance::default() };
    assert!(close(spread::actor_spread(&r, &t, &w, &hip), 0.3));
    let running = Stance { moving: true, running: true, ..hip.clone() };
    assert!(close(spread::actor_spread(&r, &t, &w, &running), 0.5));
    assert!(close(spread::deviation_deg(1.0, 0.0), 5.955));
    // The deviation is a diameter: the cone's half-angle is half of it.
    assert!(close(spread::cone_deg(1.0, 0.0, &fnv_combat::data::WeaponStats::default(), &[]), 5.955 / 2.0));
    assert_eq!(spread::spread_dir([1.0, 0.0, 0.0], 0.0, 0.5, 0.5), [1.0, 0.0, 0.0]);
    for k in 0..20 {
        let d = spread::spread_dir([1.0, 0.0, 0.0], 10.0, k as f32 / 20.0, 0.37);
        assert!(d[0].acos().to_degrees() <= 10.0 + 1e-3);
    }
    let mut kick = Kick::default();
    let mut rng = fnv_combat::rng::SplitMix(7);
    let mut rnd = || rng.next_f32();
    let up = kick.shot(&t, &gun(20), 0.5, &mut rnd);
    assert!(up.pitch_deg > 0.0 && kick.bloom > 0.0);
    kick.recover(&t, 5.0);
    assert!(kick.settled());
}

/// Shots closer together than the settle delay build bloom and climb; once the trigger
/// rests, bloom waits for the delay and then shrinks away. The climb never comes back
/// by itself: pulling the view down is the player's.
#[test]
fn continuous_fire_blooms_and_climbs_then_settles() {
    let t = CombatTuning::default();
    let w = gun(20);
    let mut kick = Kick::default();
    let mut rng = fnv_combat::rng::SplitMix(11);
    let mut rnd = || rng.next_f32();
    let dt = 1.0 / 60.0;
    let (mut climb, mut side) = (0.0f32, 0.0f32);
    let mut blooms = Vec::new();
    for _ in 0..8 {
        let k = kick.shot(&t, &w, 0.5, &mut rnd);
        climb += k.pitch_deg;
        side += k.yaw_deg;
        blooms.push(kick.bloom);
        // 0.55 s between shots, a pistol at its fastest: longer than the bare settle
        // delay, shorter than the weapon's interval plus it.
        for _ in 0..33 {
            kick.recover(&t, dt);
        }
    }
    assert!(blooms.windows(2).all(|p| p[1] >= p[0]), "bloom only grows during the burst: {blooms:?}");
    assert!(blooms[7] > 4.0 * blooms[0], "eight shots bloom well past one: {blooms:?}");
    assert!(spread::deviation_deg(kick.bloom, 0.0) > 3.0, "the burst widens the cone by degrees");
    assert_eq!(kick.burst, 8);
    assert!(climb > 8.0, "the view climbs over the burst: {climb}");
    assert!(side.abs() < climb, "mostly up: {side} sideways for {climb} up");
    assert!((kick.climb_deg - climb).abs() < 1e-3 && (kick.climb_yaw_deg - side).abs() < 1e-3);
    // Resting: the delay first, then bloom shrinks away; the climb stays.
    let before = kick.bloom;
    kick.recover(&t, kick.settle_after - kick.since_shot - 0.01);
    assert_eq!(kick.bloom, before);
    for _ in 0..120 {
        kick.recover(&t, dt);
    }
    assert!(kick.settled(), "settled within two seconds: {kick:?}");
    assert!((kick.climb_deg - climb).abs() < 1e-3, "nothing pulls the view back");
    // The next burst starts from nothing.
    kick.shot(&t, &w, 0.5, &mut rnd);
    assert_eq!(kick.burst, 1);
    assert!(kick.climb_deg < climb / 4.0);
}

/// Two bursts climb differently (the lean and jitter are random); automatics kick and
/// bloom less per shot than a semi-automatic of the same damage.
#[test]
fn recoil_patterns_differ_and_automatics_kick_less() {
    let t = CombatTuning::default();
    let burst = |seed: u64, w: &WeaponStats| {
        let mut kick = Kick::default();
        let mut rng = fnv_combat::rng::SplitMix(seed);
        let mut rnd = || rng.next_f32();
        (0..6).map(|_| kick.shot(&t, w, 0.5, &mut rnd).yaw_deg).collect::<Vec<_>>()
    };
    assert_ne!(burst(1, &gun(20)), burst(2, &gun(20)));
    let semi = gun(20);
    let auto = WeaponStats { automatic: true, ..gun(20) };
    let (mut a, mut b) = (Kick::default(), Kick::default());
    let mut rnd = || 0.5;
    let ka = a.shot(&t, &semi, 0.5, &mut rnd);
    let kb = b.shot(&t, &auto, 0.1, &mut rnd);
    assert!(kb.pitch_deg < ka.pitch_deg && b.bloom < a.bloom);
}

fn run(c: &mut Cycle, t: &CombatTuning, ticks: usize, dt: f32, trigger: impl Fn(usize) -> bool, carried: u32) -> Vec<Event> {
    (0..ticks).flat_map(|k| c.step(t, Input { trigger: trigger(k), reload: false, dt, carried })).collect()
}

fn shots(ev: &[Event]) -> usize {
    ev.iter().filter(|e| matches!(e, Event::Fired { .. })).count()
}

#[test]
fn semi_automatic_cycle_and_reload() {
    let t = CombatTuning::default();
    let w = gun(20);
    let timing = Timing::of(&w, &ClipTiming::default(), &t);
    assert!(close(timing.interval, 0.5));
    // Held: one shot only.
    let mut c = Cycle::new(&w, timing, false, 2);
    assert_eq!(shots(&run(&mut c, &t, 40, 0.05, |_| true, 10)), 1);
    // Pulled every 0.6 s for 6 s: two shots, the empty pull starts a 2 s reload (pulls
    // during it are ignored), two more shots, another reload.
    let mut c = Cycle::new(&w, timing, false, 2);
    let ev = run(&mut c, &t, 120, 0.05, |k| k % 12 == 0, 10);
    let fired = Event::Fired { rounds: 1 };
    assert_eq!(&ev[..5], &[fired.clone(), fired.clone(), Event::ReloadStarted, Event::Reloaded { taken: 2 }, fired], "{ev:?}");
    assert_eq!(shots(&ev), 4);
    // Nothing carried: one dry click per pull.
    let mut c = Cycle::new(&w, timing, false, 0);
    let ev = run(&mut c, &t, 20, 0.05, |_| true, 0);
    assert_eq!(ev, vec![Event::DryFire]);
}

#[test]
fn the_tweaks_keep_a_semi_auto_pull_and_reload_in_recovery() {
    // docs/design/tweaks.md: bSemiAutoQueue and bReloadWhileFiring.
    let t = CombatTuning::default();
    let w = WeaponStats { clip_size: 20, ..gun(20) };
    let timing = Timing::of(&w, &ClipTiming::default(), &t);
    // Pulls every 0.45 s at a 0.5 s interval: plain, each second pull lands early and
    // is lost; with a 0.1 s queue it is kept.
    let pulls = |k: usize| k.is_multiple_of(9);
    let mut plain = Cycle::new(&w, timing, false, 20);
    let mut queued = Cycle::new(&w, timing, false, 20);
    queued.semi_auto_queue = 0.1;
    let a = shots(&run(&mut plain, &t, 60, 0.05, pulls, 0));
    let b = shots(&run(&mut queued, &t, 60, 0.05, pulls, 0));
    assert!(b > a, "plain {a}, queued {b}");
    // A reload asked for while the shot recovers starts at once.
    let mut c = Cycle::new(&w, timing, false, 5);
    c.reload_in_recovery = true;
    let mut ev = c.step(&t, Input { trigger: true, reload: false, dt: 0.05, carried: 10 });
    for _ in 0..10 {
        if ev.iter().any(|e| matches!(e, Event::Fired { .. })) {
            break;
        }
        ev.extend(c.step(&t, Input { trigger: false, reload: false, dt: 0.01, carried: 10 }));
    }
    ev.extend(c.step(&t, Input { trigger: false, reload: true, dt: 0.01, carried: 10 }));
    assert!(ev.contains(&Event::ReloadStarted), "{ev:?}");
}

#[test]
fn every_fourth_shot_is_free_with_a_regenerate_ammo_shots_mod() {
    // weapon-data-fields.md §5: shots 4 and 8 of eight use no ammunition.
    let t = CombatTuning::default();
    let w = WeaponStats { free_fourth_shot: true, clip_size: 20, ..gun(20) };
    let timing = Timing::of(&w, &ClipTiming::default(), &t);
    let mut c = Cycle::new(&w, timing, false, 20);
    let ev = run(&mut c, &t, 200, 0.05, |k| k % 12 == 0, 0);
    let rounds: Vec<u32> = ev.iter().filter_map(|e| if let Event::Fired { rounds } = e { Some(*rounds) } else { None }).take(8).collect();
    assert_eq!(rounds, vec![1, 1, 1, 0, 1, 1, 1, 0], "{ev:?}");
    let used: u32 = ev.iter().map(|e| if let Event::Fired { rounds } = e { *rounds } else { 0 }).sum();
    assert_eq!(c.magazine, 20 - used);
}

#[test]
fn a_projectile_speed_mod_speeds_all_but_hitscan_shots() {
    let p = ProjectileStats { speed: 1000.0, range: 5000.0, ..ProjectileStats::default() };
    let mut shot = Projectile::spawn(&p, None, [0.0; 3], [1.0, 0.0, 0.0], None, 0.0, &CombatTuning::default());
    shot.speed_up(1.5);
    assert!(close(shot.speed, 1500.0) && close(shot.vel[0], 1500.0));
    let beam = ProjectileStats { flags: vec!["Hitscan".into()], ..p.clone() };
    let mut laser = Projectile::spawn(&beam, None, [0.0; 3], [1.0, 0.0, 0.0], None, 0.0, &CombatTuning::default());
    laser.speed_up(1.5);
    assert!(close(laser.speed, 1000.0));
}

#[test]
fn semi_automatic_shots_follow_the_clips_next_attack_key() {
    // The 9mm pistol's `1hpattackright.kf` (`a:R` at 0.400 s, end 0.500 s) at attack
    // multiplier 1.25: the record's Attack Shots/Sec, 3.125.
    let t = CombatTuning::default();
    let pistol = WeaponStats { attack_mult: 1.25, anim_mult: 1.0, ..gun(16) };
    let clips = ClipTiming { attack_len: Some(0.5), hit_time: Some(0.033), next_attack: Some((0.4, NextAttack::Right)), ..Default::default() };
    assert!(close(Timing::of(&pistol, &clips, &t).interval, 1.0 / 3.125));
    // The varmint rifle's `2hrattack8.kf` (`a:8` at 1.267 s, end 1.333 s), 1.4 × 1.1.
    let varmint = WeaponStats { attack_mult: 1.4, anim_mult: 1.1, ..gun(18) };
    let clips = ClipTiming { attack_len: Some(1.333), next_attack: Some((1.2667, NextAttack::Numbered(8))), ..Default::default() };
    assert!((Timing::of(&varmint, &clips, &t).interval - 1.0 / 1.2158).abs() < 1e-3);
    // No `a:` key: the clip's length.
    let clips = ClipTiming { attack_len: Some(0.5), ..Default::default() };
    assert!(close(Timing::of(&pistol, &clips, &t).interval, 0.4));
}

#[test]
fn a_jam_holds_the_weapon_until_it_clears() {
    let t = CombatTuning::default();
    let w = gun(20);
    let timing = Timing::of(&w, &ClipTiming::default(), &t);
    let mut c = Cycle::new(&w, timing, false, 5);
    assert_eq!(shots(&run(&mut c, &t, 10, 0.05, |_| true, 10)), 1);
    // The shot jammed it for a second: pulls and reloads do nothing meanwhile.
    c.jam(1.0);
    assert!(c.jammed());
    let ev = run(&mut c, &t, 18, 0.05, |k| k % 2 == 0, 10);
    assert_eq!(shots(&ev), 0, "{ev:?}");
    assert!(!ev.contains(&Event::ReloadStarted));
    let ev = c.step(&t, Input { trigger: false, reload: true, dt: 0.0, carried: 10 });
    assert!(ev.is_empty() && c.jammed());
    // Cleared after the second; the next pull fires.
    let ev = run(&mut c, &t, 4, 0.05, |_| false, 10);
    assert_eq!(ev, vec![Event::Cleared]);
    assert!(!c.jammed());
    assert_eq!(shots(&run(&mut c, &t, 10, 0.05, |_| true, 10)), 1);
    // A weapon without ammo (melee) never jams.
    let mut m = Cycle::new(&w, timing, true, 0);
    m.jam(1.0);
    assert!(!m.jammed());
}

#[test]
fn jam_chance_grows_below_three_quarters() {
    let t = CombatTuning::default();
    assert_eq!(fnv_combat::condition::jam_chance(&t, 1.0), 0.0);
    assert_eq!(fnv_combat::condition::jam_chance(&t, 0.75), 0.0);
    let (a, b, z) = (fnv_combat::condition::jam_chance(&t, 0.6), fnv_combat::condition::jam_chance(&t, 0.3), fnv_combat::condition::jam_chance(&t, 0.0));
    assert!(0.0 < a && a < b && b < z, "{a} {b} {z}");
    assert!(close(z, t.jam_chance_max));
    // Kept low: a weapon at half condition jams fewer than one shot in a hundred.
    assert!(fnv_combat::condition::jam_chance(&t, 0.5) < 0.01);
}

#[test]
fn two_handed_weapons_by_animation_type() {
    let w = |t: &str| WeaponStats { anim_type: Some(t.to_string()), ..WeaponStats::default() };
    assert!(w("Rifle - Ballistic (2 Hand)").two_handed() && w("Melee (2 Hand)").two_handed() && w("Launcher (2 Hand)").two_handed());
    assert!(!w("Pistol - Ballistic (1 Hand)").two_handed() && !w("Thrown (1 Hand)").two_handed() && !WeaponStats::default().two_handed());
}

#[test]
fn automatic_fire_rate_and_regen() {
    let t = CombatTuning::default();
    let w = WeaponStats { automatic: true, fire_rate: 10.0, clip_size: 30, ..gun(10) };
    let mut c = Cycle::new(&w, Timing::of(&w, &ClipTiming::default(), &t), false, 30);
    let n = shots(&run(&mut c, &t, 60, 1.0 / 60.0, |_| true, 0));
    assert!((10..=11).contains(&n), "{n}");
    // Regenerating weapon: +1 per regen_rate seconds, never reloads by hand.
    let r = WeaponStats { regen_rate: 1.0, clip_size: 5, ..gun(10) };
    let mut c = Cycle::new(&r, Timing::of(&r, &ClipTiming::default(), &t), false, 3);
    let ev: Vec<Event> = (0..150).flat_map(|_| c.step(&t, Input { trigger: false, reload: true, dt: 0.02, carried: 99 })).collect();
    assert_eq!(c.magazine, 5);
    assert!(!ev.contains(&Event::ReloadStarted));
    // A melee weapon swings without ammo.
    let blade = WeaponStats { anim_type: Some("Melee (1 Hand)".into()), clip_size: 0, ..gun(10) };
    let mut c = Cycle::new(&blade, Timing::of(&blade, &ClipTiming::default(), &t), true, 0);
    assert_eq!(shots(&run(&mut c, &t, 10, 0.05, |k| k == 0, 0)), 1);
}

/// #2739: an NPC's semi-automatic gun is spaced by its attack clip in the actor's rig, as
/// the player's: a 2.0 s clip without an `a:` key gives a shot every 2.0 s, not 1 ÷ Attack
/// Shots/Sec (0.63 s for 1.5789); with an `a:` key (the sentry bot's `a:R` at 0.967 s),
/// from that key. Without a clip, 1 ÷ Attack Shots/Sec as before. An automatic weapon
/// keeps its Fire Rate, its clip left out. The NPC's timing has no wind-up (its clip
/// starts with the shot).
#[test]
fn an_npcs_semi_automatic_gun_follows_its_attack_clip() {
    let t = CombatTuning::default();
    let launcher = WeaponStats { clip_size: 1, attack_shots_per_sec: 1.578_947_4, attack_mult: 1.0, anim_mult: 1.0, embedded: true, ..gun(20) };
    let interval = |w: &WeaponStats, clips: &ClipTiming| Timing::of(w, &fnv_combat::attack::npc_clip_timing(clips, w.automatic), &t);
    let clip = ClipTiming { attack_len: Some(2.0), hit_time: Some(0.067), reload_len: Some(1.5), ..ClipTiming::default() };
    let timed = interval(&launcher, &clip);
    assert!((timed.interval - 2.0).abs() < 1e-5, "{timed:?}");
    assert_eq!((timed.fire_delay, timed.reload), (0.0, 1.5));
    let keyed = interval(&launcher, &ClipTiming { next_attack: Some((0.967, NextAttack::Right)), ..clip });
    assert!((keyed.interval - 0.967).abs() < 1e-5, "{keyed:?}");
    let unknown = interval(&launcher, &ClipTiming::default());
    assert!((unknown.interval - 1.0 / 1.578_947_4).abs() < 1e-5, "{unknown:?}");
    // The cycle: a trigger pulled whenever it is ready fires every 2.0 s, not every 0.63 s.
    let mut c = Cycle::new(&launcher, timed, true, 1);
    let (dt, mut held, mut at, mut times) = (1.0 / 120.0, false, 0.0f32, Vec::new());
    while at < 9.0 {
        let trigger = !held;
        held = trigger;
        if c.step(&t, Input { trigger, reload: false, dt, carried: 0 }).iter().any(|e| matches!(e, Event::Fired { .. })) {
            times.push(at);
        }
        at += dt;
    }
    let gaps: Vec<f32> = times.windows(2).map(|p| p[1] - p[0]).collect();
    assert!(gaps.len() >= 3 && gaps.iter().all(|g| (g - 2.0).abs() < 0.03), "{gaps:?}");
    // An automatic weapon: its Fire Rate, whatever its attack clip.
    let minigun = WeaponStats { automatic: true, fire_rate: 15.0, clip_size: 255, attack_shots_per_sec: 15.0, ..gun(9) };
    let auto = interval(&minigun, &ClipTiming { attack_len: Some(2.0), next_attack: Some((0.5, NextAttack::Right)), spin_up: Some(0.9), ..ClipTiming::default() });
    assert!((auto.interval - 1.0 / 15.0).abs() < 1e-5 && auto.spin_up == 0.0, "{auto:?}");
    assert_eq!(auto.interval, interval(&minigun, &ClipTiming::default()).interval);
}

/// A minigun: its attack clip puts the first shot 0.967 s in (`2hhattackspin.kf`).
#[test]
fn a_spinning_weapon_fires_after_its_spin_up() {
    let t = CombatTuning::default();
    let w = WeaponStats { automatic: true, fire_rate: 20.0, clip_size: 240, ..gun(10) };
    let clips = ClipTiming { spin_up: Some(0.967), ..Default::default() };
    let timing = Timing::of(&w, &clips, &t);
    assert!(close(timing.spin_up, 0.967));
    let dt = 1.0 / 60.0;
    // Held for 2 s: the spin-up starts at once, nothing leaves for 0.967 s, then 20 a
    // second for the rest.
    let mut c = Cycle::new(&w, timing, false, 240);
    let ev = run(&mut c, &t, 120, dt, |_| true, 0);
    assert_eq!(ev[0], Event::SpinUp);
    assert_eq!(ev.iter().filter(|e| **e == Event::SpinUp).count(), 1);
    let mut c = Cycle::new(&w, timing, false, 240);
    assert_eq!(shots(&run(&mut c, &t, 57, dt, |_| true, 0)), 0, "0.95 s in");
    assert_eq!(shots(&run(&mut c, &t, 3, dt, |_| true, 0)), 1, "1 s in");
    let n = shots(&run(&mut c, &t, 60, dt, |_| true, 0));
    assert!((19..=21).contains(&n), "{n}");
    // Let go half way through the spin-up: nothing leaves, and the next pull spins up
    // again from the start.
    let mut c = Cycle::new(&w, timing, false, 240);
    let ev = run(&mut c, &t, 60, dt, |k| k < 30, 0);
    assert_eq!(ev, vec![Event::SpinUp]);
    let ev = run(&mut c, &t, 57, dt, |_| true, 0);
    assert_eq!(ev, vec![Event::SpinUp]);
    // A pause in a burst spins up again too.
    let mut c = Cycle::new(&w, timing, false, 240);
    let ev = run(&mut c, &t, 240, dt, |k| !(90..100).contains(&k), 0);
    assert_eq!(ev.iter().filter(|e| **e == Event::SpinUp).count(), 2);
    // A semi-automatic weapon does not spin up, whatever its clip; neither does an
    // automatic one whose clip has no loop.
    let semi = gun(20);
    assert_eq!(Timing::of(&semi, &clips, &t).spin_up, 0.0);
    assert_eq!(Timing::of(&w, &ClipTiming::default(), &t).spin_up, 0.0);
    let mut c = Cycle::new(&w, Timing::of(&w, &ClipTiming::default(), &t), false, 240);
    assert_eq!(shots(&run(&mut c, &t, 2, dt, |_| true, 0)), 1, "at once");
}

/// The cycle fed as the engine feeds the player's: `total` rounds in the inventory, the
/// magazine's among them, and a shot takes its rounds from there.
fn run_carrying(c: &mut Cycle, t: &CombatTuning, ticks: usize, dt: f32, trigger: impl Fn(usize) -> bool, total: &mut u32) -> Vec<Event> {
    let mut ev = Vec::new();
    for k in 0..ticks {
        let e = c.step(t, Input { trigger: trigger(k), reload: false, dt, carried: total.saturating_sub(c.magazine) });
        for x in &e {
            if let Event::Fired { rounds } = x {
                *total -= rounds;
            }
        }
        ev.extend(e);
    }
    ev
}

/// An Ammo Swap or a script's RemoveItem during the spin-up (#1613, 196 shots from ten
/// rounds): the shot it was spinning up for does not leave.
#[test]
fn a_magazine_emptied_in_the_spin_up_fires_nothing() {
    let t = CombatTuning::default();
    let w = WeaponStats { automatic: true, fire_rate: 20.0, clip_size: 240, ..gun(10) };
    let timing = Timing::of(&w, &ClipTiming { spin_up: Some(0.967), ..Default::default() }, &t);
    let dt = 1.0 / 60.0;
    let spinning = || {
        let mut c = Cycle::new(&w, timing, false, 5);
        assert_eq!(c.step(&t, Input { trigger: true, reload: false, dt, carried: 0 }), vec![Event::SpinUp]);
        c.magazine = 0;
        c
    };

    // Five of the other type carried, trigger held: a reload, a new spin-up, those five
    // and then silence (one click per pull, and this pull was answered by the reload).
    let mut c = spinning();
    let mut total = 5;
    let ev = run_carrying(&mut c, &t, 1200, dt, |_| true, &mut total);
    assert_eq!(ev[..3], [Event::ReloadStarted, Event::Reloaded { taken: 5 }, Event::SpinUp], "{ev:?}");
    assert_eq!(shots(&ev), 5, "{ev:?}");
    assert!(!ev.contains(&Event::DryFire), "{ev:?}");
    assert_eq!((c.magazine, total), (0, 0));
    // Let go and pulled again: the click.
    assert_eq!(run_carrying(&mut c, &t, 10, dt, |k| k >= 5, &mut total), vec![Event::DryFire]);

    // Nothing carried: the pull ends in its one click, whenever the spin-up is over.
    let mut c = spinning();
    let mut total = 0;
    assert_eq!(run_carrying(&mut c, &t, 300, dt, |_| true, &mut total), vec![Event::DryFire]);

    // An Ammo Swap: the reload starts at the next step, not when the spin-up is over.
    let mut c = spinning();
    c.swap_ammo();
    assert_eq!(c.step(&t, Input { trigger: true, reload: false, dt, carried: 5 }), vec![Event::ReloadStarted]);
}

/// Ammo Swap between shots: the reload comes without another pull, and the next shot no
/// sooner than the weapon's rate allows, even when the reload is shorter than the wait.
#[test]
fn a_swap_between_shots_reloads_when_the_wait_ends() {
    let t = CombatTuning::default();
    let w = WeaponStats { clip_size: 12, ..gun(20) };
    let timing = Timing { interval: 3.0, fire_delay: 0.0, reload: 1.0, spin_up: 0.0, rounds: None, attach: None };
    let dt = 0.05;
    let mut c = Cycle::new(&w, timing, false, 5);
    assert_eq!(run(&mut c, &t, 2, dt, |k| k == 0, 0), vec![Event::Fired { rounds: 1 }]);
    assert!(matches!(c.phase, Phase::Cooldown { .. }));
    c.swap_ammo();
    // Still waiting 2.8 s on: 2.9 s of the 3 s since the shot.
    assert!(run(&mut c, &t, 56, dt, |_| false, 5).is_empty());
    assert_eq!(c.magazine, 0);
    // The wait ends and the reload follows, the trigger untouched.
    assert_eq!(run(&mut c, &t, 6, dt, |_| false, 5), vec![Event::ReloadStarted]);
    assert_eq!(run(&mut c, &t, 24, dt, |_| false, 5), vec![Event::Reloaded { taken: 5 }]);
    assert_eq!(c.magazine, 5);
    assert_eq!(shots(&run(&mut c, &t, 2, dt, |k| k == 0, 0)), 1);

    // An automatic weapon held through it reloads and does not click.
    let w = WeaponStats { automatic: true, fire_rate: 2.0, clip_size: 12, ..gun(20) };
    let mut c = Cycle::new(&w, Timing::of(&w, &ClipTiming::default(), &t), false, 5);
    assert_eq!(c.step(&t, Input { trigger: true, reload: false, dt, carried: 0 }), vec![Event::Fired { rounds: 1 }]);
    c.swap_ammo();
    let ev = run(&mut c, &t, 20, dt, |_| true, 5);
    assert_eq!(ev, vec![Event::ReloadStarted], "{ev:?}");

    // Nothing to reload into: the wish is dropped and does not fire off later.
    let w = WeaponStats { clip_size: 12, ..gun(20) };
    let mut c = Cycle::new(&w, timing, false, 5);
    assert_eq!(run(&mut c, &t, 2, dt, |k| k == 0, 0), vec![Event::Fired { rounds: 1 }]);
    c.swap_ammo();
    assert!(run(&mut c, &t, 70, dt, |_| false, 0).is_empty());
    assert!(run(&mut c, &t, 10, dt, |_| false, 5).is_empty());
}

/// Ammo Swap during a reload: it goes on, into the new ammo, on its own clock; while the
/// weapon closes or is jammed the magazine stays empty until the next pull.
#[test]
fn a_swap_leaves_a_reload_and_a_jam_as_they_are() {
    let t = CombatTuning::default();
    let w = WeaponStats { clip_size: 12, ..gun(20) };
    let timing = Timing { interval: 0.5, fire_delay: 0.0, reload: 1.0, spin_up: 0.0, rounds: None, attach: None };
    let dt = 0.05;
    let reload = Input { trigger: false, reload: true, dt, carried: 5 };

    // The whole magazine: the time left stands, and the new ammo goes in.
    let mut c = Cycle::new(&w, timing, false, 4);
    assert_eq!(c.step(&t, reload), vec![Event::ReloadStarted]);
    assert!(run(&mut c, &t, 10, dt, |_| false, 5).is_empty());
    c.swap_ammo();
    let ev = run(&mut c, &t, 12, dt, |_| false, 5);
    assert_eq!(ev, vec![Event::Reloaded { taken: 5 }], "{ev:?}");
    assert_eq!(c.magazine, 5);

    // Round by round, in the closing: nothing more goes in, and the next pull reloads.
    let rounds = Timing { rounds: Some(RoundReload { open: 0.1, round: 0.1, close: 0.5 }), ..timing };
    let mut c = Cycle::new(&w, rounds, false, 11);
    assert_eq!(c.step(&t, reload), vec![Event::ReloadStarted]);
    let mut closing = run(&mut c, &t, 6, dt, |_| false, 5);
    closing.retain(|e| matches!(e, Event::ReloadClosing { .. }));
    assert_eq!(closing.len(), 1);
    c.swap_ammo();
    assert_eq!(c.magazine, 0);
    let ev = run(&mut c, &t, 12, dt, |_| false, 5);
    assert!(ev.iter().any(|e| matches!(e, Event::Reloaded { .. })), "{ev:?}");
    assert_eq!(c.magazine, 0);
    assert_eq!(c.step(&t, Input { trigger: true, reload: false, dt, carried: 5 }), vec![Event::ReloadStarted]);

    // Jammed: cleared with no reload, then the next pull reloads.
    let mut c = Cycle::new(&w, timing, false, 5);
    c.jam(0.2);
    c.swap_ammo();
    assert_eq!(run(&mut c, &t, 8, dt, |_| false, 5), vec![Event::Cleared]);
    assert_eq!(c.step(&t, Input { trigger: true, reload: false, dt, carried: 5 }), vec![Event::ReloadStarted]);
}

/// A gun whose shot waits for the attack clip's `Hit` key, emptied in that wait.
#[test]
fn a_magazine_emptied_in_the_wind_up_fires_nothing() {
    let t = CombatTuning::default();
    let w = WeaponStats { clip_size: 12, ..gun(20) };
    let timing = Timing { interval: 0.5, fire_delay: 0.2, reload: 1.0, spin_up: 0.0, rounds: None, attach: None };
    let dt = 0.05;
    let winding = |w: &WeaponStats, magazine: u32| {
        let mut c = Cycle::new(w, timing, false, 5);
        assert!(c.step(&t, Input { trigger: true, reload: false, dt, carried: 0 }).is_empty());
        assert!(matches!(c.phase, Phase::WindUp { .. }));
        c.magazine = magazine;
        c
    };

    // Nothing carried, the trigger let go in the wait: still that pull's click.
    let mut c = winding(&w, 0);
    assert_eq!(run(&mut c, &t, 10, dt, |_| false, 0), vec![Event::DryFire]);
    assert_eq!(c.magazine, 0);
    // Rounds carried: the reload an empty pull starts.
    let mut c = winding(&w, 0);
    let ev = run(&mut c, &t, 10, dt, |_| false, 5);
    assert_eq!(ev[0], Event::ReloadStarted, "{ev:?}");
    assert_eq!(shots(&ev), 0, "{ev:?}");
    // Fewer rounds left than a shot uses: no shot and nothing wraps.
    let two = WeaponStats { ammo_use: 2, ..w.clone() };
    let mut c = winding(&two, 1);
    assert_eq!(run(&mut c, &t, 10, dt, |_| false, 0), vec![Event::DryFire]);
    assert_eq!(c.magazine, 1);
}

/// The fourth shot of a "Regenerate Ammo (shots)" mod takes no round, so an empty magazine
/// does not stop it once it is on its way.
#[test]
fn a_free_shot_leaves_from_an_emptied_magazine() {
    let t = CombatTuning::default();
    let w = WeaponStats { free_fourth_shot: true, clip_size: 12, ..gun(20) };
    let timing = Timing { interval: 0.5, fire_delay: 0.2, reload: 1.0, spin_up: 0.0, rounds: None, attach: None };
    let mut c = Cycle::new(&w, timing, false, 4);
    assert_eq!(shots(&run(&mut c, &t, 36, 0.05, |k| k % 12 == 0, 0)), 3);
    assert_eq!(c.magazine, 1);
    assert!(c.step(&t, Input { trigger: true, reload: false, dt: 0.05, carried: 0 }).is_empty());
    c.magazine = 0;
    assert_eq!(run(&mut c, &t, 8, 0.05, |_| false, 0), vec![Event::Fired { rounds: 0 }]);
    assert_eq!(c.magazine, 0);
    // A shot that did not leave is not counted towards the next free one.
    let mut c = Cycle::new(&w, timing, false, 4);
    assert_eq!(shots(&run(&mut c, &t, 24, 0.05, |k| k % 12 == 0, 0)), 2);
    assert!(c.step(&t, Input { trigger: true, reload: false, dt: 0.05, carried: 0 }).is_empty());
    c.magazine = 0;
    assert_eq!(run(&mut c, &t, 8, 0.05, |_| false, 0), vec![Event::DryFire]);
    c.magazine = 4;
    let ev = run(&mut c, &t, 24, 0.05, |k| k % 12 == 0, 0);
    let rounds: Vec<u32> = ev.iter().filter_map(|e| if let Event::Fired { rounds } = e { Some(*rounds) } else { None }).collect();
    assert_eq!(rounds, vec![1, 0], "shots 3 and 4: {ev:?}");
}

/// Per-Bullet Looping Reloads: a round a press, presses while it loads queue more, up
/// to the most.
#[test]
fn per_bullet_reloads_load_what_the_presses_queued() {
    let t = CombatTuning::default();
    let w = WeaponStats { clip_size: 6, ..gun(26) };
    let clips = ClipTiming { reload_len: Some(1.767), reload_hit: Some(0.633), reload_open_len: Some(1.067), ..Default::default() };
    let timing = Timing::of(&w, &clips, &t);
    let dt = 0.01;
    let run = |presses: &[usize], most: u32| {
        let mut c = Cycle::new(&w, timing, false, 0);
        c.reload_presses = Some((1, most));
        let ev: Vec<Event> = (0..800).flat_map(|k| c.step(&t, Input { trigger: false, reload: presses.contains(&k), dt, carried: 20 })).collect();
        (c.magazine, ev)
    };
    // One press: one round, then it closes.
    let (mag, ev) = run(&[0], 0);
    assert_eq!(mag, 1);
    assert!(ev.contains(&Event::Reloaded { taken: 1 }));
    // Two more presses while it opens and loads: three rounds.
    assert_eq!(run(&[0, 50, 120], 0).0, 3);
    // At most two queued.
    assert_eq!(run(&[0, 50, 120, 130], 2).0, 2);
    // Off: the whole magazine.
    let mut c = Cycle::new(&w, timing, false, 0);
    for k in 0..800 {
        c.step(&t, Input { trigger: false, reload: k == 0, dt, carried: 20 });
    }
    assert_eq!(c.magazine, 6);
}

/// Reload While Firing with Per-Bullet Looping Reloads (docs/design/tweaks.md): a press
/// while the shot recovers starts a one-round reload, counted once, and a pull in the
/// same tick does not stop it.
#[test]
fn a_reload_in_recovery_takes_its_press_once(){
    let t = CombatTuning::default();
    let w = WeaponStats { clip_size: 6, ..gun(26) };
    let clips = ClipTiming { reload_len: Some(1.767), reload_hit: Some(0.633), reload_open_len: Some(1.067), ..Default::default() };
    let timing = Timing::of(&w, &clips, &t);
    let dt = 0.01;
    for pull in [false, true] {
        let mut c = Cycle::new(&w, timing, false, 1);
        c.reload_presses = Some((1, 0));
        c.reload_in_recovery = true;
        let mut ev = c.step(&t, Input { trigger: true, reload: false, dt, carried: 20 });
        for _ in 0..100 {
            if ev.iter().any(|e| matches!(e, Event::Fired { .. })) {
                break;
            }
            ev.extend(c.step(&t, Input { trigger: false, reload: false, dt, carried: 20 }));
        }
        assert!(ev.iter().any(|e| matches!(e, Event::Fired { .. })), "{ev:?}");
        assert_eq!(c.magazine, 0);
        // Still recovering: the press (and the pull) come now.
        let ev = c.step(&t, Input { trigger: pull, reload: true, dt, carried: 20 });
        assert!(ev.contains(&Event::ReloadStarted), "pull {pull}: {ev:?}");
        for _ in 0..800 {
            c.step(&t, Input { trigger: false, reload: false, dt, carried: 20 });
        }
        assert_eq!(c.magazine, 1, "pull {pull}");
    }
}

/// The .357 revolver's clips (`1hpreloadxstart.kf` 1.067 s; `1hpreloadx.kf` 1.767 s,
/// `Hit` at 0.633 s): it opens, loads a round per pass up to `Hit`, and closes.
#[test]
fn a_revolver_loads_round_by_round() {
    let t = CombatTuning::default();
    let w = WeaponStats { clip_size: 6, ..gun(26) };
    let clips = ClipTiming { reload_len: Some(1.767), reload_hit: Some(0.633), reload_open_len: Some(1.067), ..Default::default() };
    let timing = Timing::of(&w, &clips, &t);
    let r = timing.rounds.expect("round by round");
    assert!(close(r.open, 1.067) && close(r.round, 0.633) && close(r.close, 1.134), "{r:?}");
    // Without the opening clip, or without `Hit`: the whole magazine at once.
    assert_eq!(Timing::of(&w, &ClipTiming { reload_open_len: None, ..clips }, &t).rounds, None);
    assert_eq!(Timing::of(&w, &ClipTiming { reload_hit: None, ..clips }, &t).rounds, None);
    let dt = 0.01;
    let steps = |c: &mut Cycle, from: usize, ticks: usize, pull: &dyn Fn(usize) -> bool, carried: u32| -> Vec<Event> {
        (from..from + ticks).flat_map(|k| c.step(&t, Input { trigger: pull(k), reload: k == 0, dt, carried })).collect()
    };
    let rounds = |ev: &[Event]| ev.iter().filter(|e| **e == Event::RoundLoaded).count();

    // Four missing: open, four rounds, close: 1.067 + 4 × 0.633 + 1.134 = 4.73 s.
    let mut c = Cycle::new(&w, timing, false, 2);
    let ev = steps(&mut c, 0, 460, &|_| false, 20);
    assert_eq!(c.magazine, 6);
    assert!(!ev.iter().any(|e| matches!(e, Event::Reloaded { .. })), "closed too soon: {ev:?}");
    let ev2 = steps(&mut c, 460, 20, &|_| false, 20);
    assert_eq!(ev2, vec![Event::Reloaded { taken: 4 }]);
    assert_eq!(&ev[..3], &[Event::ReloadStarted, Event::ReloadRound, Event::RoundLoaded]);
    assert_eq!(rounds(&ev), 4);
    assert_eq!(ev.last(), Some(&Event::ReloadClosing { stopped: false }));

    // Empty, the trigger pulled 2 s in (the second round is in hand): it goes in and the
    // revolver closes (to 3.47 s); that pull fires nothing (owner, 2026-10-11).
    let mut c = Cycle::new(&w, timing, false, 0);
    let ev = steps(&mut c, 0, 340, &|k| k == 200, 20);
    assert_eq!(rounds(&ev), 2);
    assert!(ev.contains(&Event::ReloadClosing { stopped: true }));
    assert_eq!(shots(&ev), 0, "not before it is closed");
    let ev = steps(&mut c, 340, 20, &|_| false, 20);
    assert_eq!(ev, vec![Event::Reloaded { taken: 2 }]);
    assert_eq!(c.magazine, 2);
    // Pulled again once it is closed: it fires.
    let ev = steps(&mut c, 360, 20, &|k| k == 365, 20);
    assert_eq!(shots(&ev), 1);

    // A second pull while it closes, within the grace of its end (3.47 s): the shot
    // follows the close. Earlier than the grace: dropped.
    let mut c = Cycle::new(&w, timing, false, 0);
    let ev = steps(&mut c, 0, 360, &|k| k == 200 || k == 330, 20);
    assert_eq!(ev[ev.len() - 2..], [Event::Reloaded { taken: 2 }, Event::Fired { rounds: 1 }]);
    let mut c = Cycle::new(&w, timing, false, 0);
    let ev = steps(&mut c, 0, 360, &|k| k == 200 || k == 300, 20);
    assert_eq!(shots(&ev), 0, "the pull 0.47 s before the close ended is dropped");

    // Pulled while it opens: the first round still goes in, then it closes, no shot.
    let mut c = Cycle::new(&w, timing, false, 0);
    let ev = steps(&mut c, 0, 300, &|k| k == 50, 20);
    assert_eq!(rounds(&ev), 1);
    assert_eq!(shots(&ev), 0);

    // One round carried: it goes in and the revolver closes.
    let mut c = Cycle::new(&w, timing, false, 0);
    let ev = steps(&mut c, 0, 300, &|_| false, 1);
    assert_eq!((rounds(&ev), c.magazine), (1, 1));
    assert!(ev.contains(&Event::Reloaded { taken: 1 }));

    // An empty pull starts the reload; that pull does not stop it.
    let mut c = Cycle::new(&w, timing, false, 0);
    let ev: Vec<Event> = (0..800).flat_map(|k| c.step(&t, Input { trigger: k < 3, reload: false, dt, carried: 20 })).collect();
    assert_eq!((rounds(&ev), shots(&ev)), (6, 0));
}

/// An NPC's revolver as the combat AI drives it (nvforever `npc_combat`): a pull on
/// every other tick, none while it reloads, a reload asked for once it is empty. It fires
/// its six, loads all six round by round and fires again; pulling through the reload
/// would close the cylinder after one round.
#[test]
fn a_gunman_that_holds_fire_while_reloading_loads_every_round() {
    let t = CombatTuning::default();
    let w = WeaponStats { clip_size: 6, ..gun(26) };
    let clips = ClipTiming { reload_len: Some(1.767), reload_hit: Some(0.633), reload_open_len: Some(1.067), ..Default::default() };
    let dt = 0.01;
    let drive = |guard: bool| {
        let mut c = Cycle::new(&w, Timing::of(&w, &clips, &t), false, 6);
        let mut pulled = false;
        let mut ev = Vec::new();
        let mut loaded = Vec::new();
        for _ in 0..1600 {
            let reloading = c.reloading();
            let trigger = !(guard && reloading) && !pulled;
            pulled = trigger;
            let empty = c.magazine < c.ammo_use;
            let e = c.step(&t, Input { trigger, reload: guard && empty && !reloading, dt, carried: u32::MAX / 2 });
            loaded.extend(e.iter().filter_map(|e| match e {
                Event::Reloaded { taken } => Some(*taken),
                _ => None,
            }));
            ev.extend(e);
        }
        (shots(&ev), loaded)
    };
    let (fired, loaded) = drive(true);
    assert_eq!(loaded.first(), Some(&6), "{loaded:?}");
    assert!(fired > 6, "fires again after the reload: {fired}");
    let (_, loaded) = drive(false);
    assert_eq!(loaded.first(), Some(&1), "pulled through, the reload stops after a round: {loaded:?}");
}

/// Floor at z = 0, spherical actors, and an optional small-transparent fence at x = 500.
struct Toy {
    actors: Vec<(ActorId, V3, f32)>,
    fence: bool,
    wall: Option<f32>,
}

impl CombatWorld for Toy {
    fn ray(&self, from: V3, to: V3, ignore: &[ActorId]) -> Option<WorldHit> {
        let d = vec::sub(to, from);
        let len = vec::len(d);
        let dir = vec::normalize(d);
        let mut best: Option<WorldHit> = None;
        let mut take = |h: WorldHit| {
            if h.distance >= 0.0 && h.distance <= len && best.as_ref().is_none_or(|b| h.distance < b.distance) {
                best = Some(h);
            }
        };
        if dir[2] < 0.0 && from[2] >= 0.0 {
            let s = -from[2] / dir[2];
            take(WorldHit { point: vec::add(from, vec::scale(dir, s)), normal: [0.0, 0.0, 1.0], distance: s, actor: None, small_transparent: false, material: None, object: None, part: None });
        }
        for (x, small) in [(500.0, true)].into_iter().filter(|_| self.fence).chain(self.wall.map(|w| (w, false))) {
            if dir[0] > 0.0 && from[0] < x {
                let s = (x - from[0]) / dir[0];
                take(WorldHit { point: vec::add(from, vec::scale(dir, s)), normal: [-1.0, 0.0, 0.0], distance: s, actor: None, small_transparent: small, material: None, object: None, part: None });
            }
        }
        for &(id, c, r) in &self.actors {
            if ignore.contains(&id) {
                continue;
            }
            let oc = vec::sub(from, c);
            let b = vec::dot(oc, dir);
            let q = vec::dot(oc, oc) - r * r;
            let disc = b * b - q;
            if disc >= 0.0 {
                let s = -b - disc.sqrt();
                let p = vec::add(from, vec::scale(dir, s));
                take(WorldHit { point: p, normal: vec::normalize(vec::sub(p, c)), distance: s, actor: Some(id), small_transparent: false, material: None, object: None, part: None });
            }
        }
        best
    }

    fn actors_near(&self, point: V3, radius: f32) -> Vec<(ActorId, V3)> {
        self.actors.iter().filter(|(_, c, r)| vec::len(vec::sub(*c, point)) <= radius + r).map(|&(id, c, _)| (id, c)).collect()
    }
}

/// A low creature (a radscorpion): its body a sphere near the floor, the centre of its
/// bounds high above it (its tail raised), and, when `parts`, its body's centre as an aim
/// point.
struct Scorpion {
    body: Toy,
    centre: V3,
    parts: bool,
}

impl CombatWorld for Scorpion {
    fn ray(&self, from: V3, to: V3, ignore: &[ActorId]) -> Option<WorldHit> {
        self.body.ray(from, to, ignore)
    }

    fn actors_near(&self, point: V3, radius: f32) -> Vec<(ActorId, V3)> {
        self.body.actors_near(point, radius).into_iter().map(|(a, _)| (a, self.centre)).collect()
    }

    fn aim_points(&self, actor: ActorId) -> Vec<V3> {
        if self.parts {
            self.body.actors.iter().filter(|x| x.0 == actor).map(|x| x.1).collect()
        } else {
            Vec::new()
        }
    }
}

#[test]
fn a_swing_reaches_a_low_creature_whose_bounds_centre_is_in_the_air() {
    let t = CombatTuning::default();
    let eye = [0.0, 0.0, 120.0];
    let world = |parts| Scorpion { body: Toy { actors: vec![(7, [60.0, 0.0, 12.0], 12.0)], fence: false, wall: None }, centre: [60.0, 0.0, 70.0], parts };
    // The ray at the centre passes over its back: aimed at its body it strikes.
    assert!(swing_reaching(&world(false), &t, eye, 0.0, [1.0, 0.0, 0.0], 75.0, 1).is_empty());
    let hits = swing_reaching(&world(true), &t, eye, 0.0, [1.0, 0.0, 0.0], 75.0, 1);
    assert_eq!(hits.iter().map(|h| h.0).collect::<Vec<_>>(), vec![7]);
    assert!(hits[0].1.point[2] < 30.0);
}

/// A grenade beside a giant radscorpion: the centre of its bounds high over its back and
/// past the radius, its body (an aim point) close by. It is caught, at its body's
/// distance; one whose body only reaches into the radius takes the edge's damage; a wall
/// still shields it.
#[test]
fn a_blast_catches_a_big_creature_by_its_body() {
    let t = CombatTuning::default();
    let e = ExplosionStats { damage: 100.0, radius: 150.0, force: 300.0, ..ExplosionStats::default() };
    let world = |parts, wall| Scorpion { body: Toy { actors: vec![(7, [120.0, 0.0, 40.0], 60.0)], fence: false, wall }, centre: [120.0, 0.0, 200.0], parts };
    let b = blast(&world(true, None), &t, &e, [40.0, 0.0, 0.0], None);
    assert_eq!(b.len(), 1);
    assert!(b[0].damage > 50.0, "{b:?}");
    let edge = blast(&world(false, None), &t, &e, [40.0, 0.0, 0.0], None);
    assert_eq!(edge.len(), 1);
    assert!(close(edge[0].damage, 25.0), "{edge:?}");
    assert!(blast(&world(true, Some(80.0)), &t, &e, [40.0, 0.0, 0.0], None).is_empty());
}

/// A living actor and a corpse that has no hit volume (a ragdoll's, #2346): the corpse is
/// among the bodies only a blast asks for.
struct Graveyard {
    living: Toy,
    corpses: Vec<(ActorId, V3, f32)>,
}

impl CombatWorld for Graveyard {
    fn ray(&self, from: V3, to: V3, ignore: &[ActorId]) -> Option<WorldHit> {
        Toy { actors: self.living.actors.iter().chain(&self.corpses).copied().collect(), fence: false, wall: self.living.wall }.ray(from, to, ignore)
    }

    fn actors_near(&self, point: V3, radius: f32) -> Vec<(ActorId, V3)> {
        self.living.actors_near(point, radius)
    }

    fn blast_targets_near(&self, point: V3, radius: f32) -> Vec<(ActorId, V3)> {
        let corpses = Toy { actors: self.corpses.clone(), fence: false, wall: None };
        self.actors_near(point, radius).into_iter().chain(corpses.actors_near(point, radius)).collect()
    }
}

/// A blast catches a settled corpse beside a living actor, each once; a wall still shields
/// the corpse. Only blasts: the corpse is not among `actors_near` (what proximity fuses and
/// swings ask), so it sets off no mine.
#[test]
fn a_blast_catches_a_corpse_without_a_hit_volume() {
    let t = CombatTuning::default();
    let e = ExplosionStats { damage: 100.0, radius: 200.0, force: 300.0, ..ExplosionStats::default() };
    let world = |wall| Graveyard { living: Toy { actors: vec![(7, [0.0, 80.0, 40.0], 20.0)], fence: false, wall }, corpses: vec![(9, [100.0, 0.0, 10.0], 30.0)] };
    let hits = blast(&world(None), &t, &e, [0.0, 0.0, 5.0], None);
    let mut caught: Vec<ActorId> = hits.iter().map(|h| h.actor).collect();
    caught.sort_unstable();
    assert_eq!(caught, vec![7, 9], "{hits:?}");
    assert!(hits.iter().all(|h| h.damage > 0.0));
    assert!(world(None).actors_near([100.0, 0.0, 10.0], 50.0).is_empty(), "no proximity fuse sees the corpse");
    let shielded = blast(&world(Some(50.0)), &t, &e, [0.0, 0.0, 5.0], None);
    assert_eq!(shielded.iter().map(|h| h.actor).collect::<Vec<_>>(), vec![7]);
}

/// A blast on the floor, the centre of an actor's bounds a little below the floor's
/// level (a low creature on a slope): the line to it runs straight into the floor. Its
/// body's parts above the floor are seen.
#[test]
fn a_blast_on_the_ground_sees_a_body_its_centre_sunk_in_it() {
    let t = CombatTuning::default();
    let e = ExplosionStats { damage: 100.0, radius: 300.0, force: 300.0, ..ExplosionStats::default() };
    let world = |parts| Scorpion { body: Toy { actors: vec![(7, [100.0, 0.0, 20.0], 20.0)], fence: false, wall: None }, centre: [100.0, 0.0, -5.0], parts };
    assert!(blast(&world(false), &t, &e, [0.0, 0.0, 0.0], None).is_empty());
    assert_eq!(blast(&world(true), &t, &e, [0.0, 0.0, 0.0], None).len(), 1);
}

/// Synthetic solid half-spaces: a ray starting on or inside one reports a hit at
/// distance zero, including rays travelling out of the ground [design: #1581 fixture].
#[derive(Clone)]
struct BlastSurfaces {
    planes: Vec<(V3, f32)>,
    actor: V3,
    impact_depth: f32,
}

impl CombatWorld for BlastSurfaces {
    fn ray(&self, from: V3, to: V3, _ignore: &[ActorId]) -> Option<WorldHit> {
        self.planes.iter().filter_map(|&(normal, offset)| {
            let start = vec::dot(from, normal) - offset;
            let end = vec::dot(to, normal) - offset;
            let share = if start <= 0.0 { 0.0 } else if end <= 0.0 { start / (start - end) } else { return None };
            let step = vec::sub(to, from);
            let point = vec::sub(vec::add(from, vec::scale(step, share)), vec::scale(normal, self.impact_depth));
            Some(WorldHit { point, normal, distance: vec::len(step) * share, actor: None, part: None, object: None, small_transparent: false, material: None })
        }).min_by(|a, b| a.distance.total_cmp(&b.distance))
    }

    fn actors_near(&self, point: V3, radius: f32) -> Vec<(ActorId, V3)> {
        if vec::len(vec::sub(self.actor, point)) <= radius { vec![(7, self.actor)] } else { vec![] }
    }
}

fn surface_blast() -> ExplosionStats {
    ExplosionStats { damage: 100.0, radius: 300.0, force: 300.0, ..ExplosionStats::default() }
}

#[test]
fn impact_blasts_clear_ground_slope_wall_and_ceiling_contacts() {
    let t = CombatTuning::default();
    let e = surface_blast();
    for (normal, actor) in [
        ([0.0, 0.0, 1.0], [30.0, 0.0, 20.0]),
        ([-0.6, 0.0, 0.8], [30.0, 0.0, 60.0]),
        ([-1.0, 0.0, 0.0], [-30.0, 0.0, 20.0]),
        ([0.0, 0.0, -1.0], [30.0, 0.0, -20.0]),
    ] {
        let world = BlastSurfaces { planes: vec![(normal, 0.0)], actor, impact_depth: 0.0 };
        for depth in [-0.01, 0.0, 0.01] {
            let center = vec::scale(normal, depth);
            let lifted = vec::add(center, [0.0, 0.0, 16.0]);
            if depth <= 0.0 {
                assert!(world.cover_ray(center, lifted).is_some(), "the fixture must report the contacted solid");
                assert!(blast(&world, &t, &e, center, None).is_empty(), "an uncleared origin is blocked");
            }
            // Normalize supplied normals so their magnitude cannot widen the clearance.
            let hits = blast(&world, &t, &e, center, Some(vec::scale(normal, 3.0)));
            assert_eq!(hits.len(), 1, "normal {normal:?}, depth {depth}: {hits:?}");
            assert_eq!(hits[0].actor, 7);
            assert!(hits[0].damage > 0.0);
            let distance = vec::len(vec::sub(actor, center));
            assert!(close(hits[0].distance, distance), "falloff uses the physical impact point");
            assert!(close(hits[0].damage, damage::explosion_damage(&t, e.damage, e.radius, distance)));
        }
        assert!(blast(&world, &t, &e, vec::scale(normal, -0.6), Some(normal)).is_empty(), "clearance remains bounded");
    }
}

#[test]
fn cleared_blasts_keep_cover_ceiling_and_radius_checks() {
    let t = CombatTuning::default();
    let e = surface_blast();
    let up = [0.0, 0.0, 1.0];
    let ground = BlastSurfaces { planes: vec![(up, 0.0)], actor: [30.0, 0.0, 20.0], impact_depth: 0.0 };
    let center = [0.0, 0.0, -0.01];
    let cover = BlastSurfaces { planes: vec![(up, 0.0), ([-1.0, 0.0, 0.0], -15.0)], ..ground };
    assert!(blast(&cover, &t, &e, center, Some(up)).is_empty(), "a wall still shields the actor");
    let ignore = ExplosionStats { flags: vec!["Ignore LOS Check".into()], ..e.clone() };
    assert_eq!(blast(&cover, &t, &ignore, center, Some(up)).len(), 1);
    // The raised origin must not cross even a low ceiling, with or without contact data.
    for normal in [None, Some(up)] {
        let ceiling = BlastSurfaces { planes: vec![(up, 0.0), ([0.0, 0.0, -1.0], -8.0)], ..ground };
        assert!(blast(&ceiling, &t, &e, [0.0, 0.0, 0.01], normal).is_empty());
    }
    let ceiling_contact = BlastSurfaces { planes: vec![([0.0, 0.0, -1.0], 0.0)], ..ground };
    assert!(blast(&ceiling_contact, &t, &e, [0.0, 0.0, 0.01], Some([0.0, 0.0, -1.0])).is_empty(), "no visibility from above a contacted ceiling");
    // Moving the LOS origin must neither admit an actor outside the physical radius
    // nor change the edge damage for one exactly on it.
    let at = [0.0, 0.0, 0.01];
    let edge = BlastSurfaces { actor: [300.0, 0.0, 0.01], ..ground.clone() };
    let hits = blast(&edge, &t, &e, at, Some(up));
    assert_eq!(hits.len(), 1);
    assert!(close(hits[0].damage, 25.0));
    let outside = BlastSurfaces { actor: [300.01, 0.0, 0.01], ..edge };
    assert!(blast(&outside, &t, &e, at, Some(up)).is_empty());
    for invalid in [None, Some([0.0; 3]), Some([f32::NAN, 0.0, 1.0]), Some([0.0, f32::INFINITY, 1.0]), Some([f32::MAX; 3])] {
        assert!(blast(&ground, &t, &e, center, invalid).is_empty(), "invalid normals leave the origin unchanged");
    }
}

#[test]
fn projectile_impact_normal_reaches_blast_visibility() {
    let t = CombatTuning::default();
    let world = BlastSurfaces { planes: vec![([0.0, 0.0, 1.0], 0.0)], actor: [30.0, 0.0, 20.0], impact_depth: 0.01 };
    for flags in [vec!["Explosion"], vec!["Explosion", "Hitscan"]] {
        let mut p = Projectile::spawn(&proj("Missile", &flags, 100.0, 0.0), Some(BOOM), [0.0, 0.0, 10.0], [0.0, 0.0, -1.0], Some(1), G, &t);
        let events = p.step(&world, 0.2);
        let (point, normal) = match events.as_slice() {
            [ProjEvent::Hit { normal: hit_normal, .. }, ProjEvent::Explode { explosion, point, normal }] => {
                assert_eq!(*explosion, BOOM);
                assert_eq!(*normal, Some(*hit_normal));
                (*point, *normal)
            }
            _ => panic!("{events:?}"),
        };
        assert_eq!(point, [0.0, 0.0, -0.01]);
        assert!(blast(&world, &t, &surface_blast(), point, None).is_empty());
        let hits = blast(&world, &t, &surface_blast(), point, normal);
        assert_eq!(hits.len(), 1);
        assert!(hits[0].damage > 0.0);
    }
}

fn proj(kind: &str, flags: &[&str], speed: f32, gravity: f32) -> ProjectileStats {
    ProjectileStats { kind: Some(kind.into()), flags: flags.iter().map(|s| s.to_string()).collect(), speed, gravity, range: 10_000.0, ..ProjectileStats::default() }
}

fn fly(p: &mut Projectile, w: &Toy, seconds: f32) -> Vec<ProjEvent> {
    let dt = 1.0 / 60.0;
    let mut out = Vec::new();
    for _ in 0..(seconds / dt) as usize {
        out.extend(p.step(w, dt));
        if !p.alive {
            break;
        }
    }
    out
}

const G: f32 = 9.81 * 70.0;
const BOOM: FormId = FormId(0x99);

#[test]
fn hitscan_beam_and_missile_drop() {
    let t = CombatTuning::default();
    let world = Toy { actors: vec![(7, [1000.0, 0.0, 100.0], 30.0)], fence: false, wall: None };
    let mut p = Projectile::spawn(&proj("Missile", &["Hitscan"], 20000.0, 1.0), None, [0.0, 0.0, 100.0], [1.0, 0.0, 0.0], Some(1), G, &t);
    match &p.step(&world, 1.0 / 60.0)[..] {
        [ProjEvent::Hit { actor: Some(7), delay, .. }] => assert!(close(*delay, 970.0 / 20000.0)),
        e => panic!("{e:?}"),
    }
    let mut b = Projectile::spawn(&proj("Beam", &[], 100.0, 0.0), None, [0.0, 0.0, 100.0], [1.0, 0.0, 0.0], Some(1), G, &t);
    assert!(matches!(&b.step(&world, 1.0 / 60.0)[..], [ProjEvent::Hit { actor: Some(7), delay, .. }] if *delay == 0.0));
    // A slow missile aimed level under gravity falls short of the actor: it hits the floor
    // after about sqrt(2 × 100 / G) = 0.54 s, near x = 540.
    let empty = Toy { actors: vec![], fence: false, wall: None };
    let mut m = Projectile::spawn(&proj("Missile", &[], 1000.0, 1.0), Some(BOOM), [0.0, 0.0, 100.0], [1.0, 0.0, 0.0], Some(1), G, &t);
    let ev = fly(&mut m, &empty, 2.0);
    match &ev[..] {
        [ProjEvent::Hit { actor: None, point, .. }, ProjEvent::Explode { explosion, .. }] => {
            assert!((point[0] - 540.0).abs() < 20.0, "{point:?}");
            assert_eq!(*explosion, BOOM);
        }
        e => panic!("{e:?}"),
    }
}

#[test]
fn flame_fence_range() {
    let t = CombatTuning::default();
    let two = Toy { actors: vec![(7, [200.0, 0.0, 100.0], 30.0), (8, [400.0, 0.0, 100.0], 30.0)], fence: false, wall: Some(800.0) };
    let mut f = Projectile::spawn(&proj("Flame", &[], 1500.0, 0.0), None, [0.0, 0.0, 100.0], [1.0, 0.0, 0.0], Some(1), G, &t);
    let hits: Vec<Option<ActorId>> = fly(&mut f, &two, 2.0).iter().filter_map(|e| if let ProjEvent::Hit { actor, .. } = e { Some(*actor) } else { None }).collect();
    assert_eq!(hits, vec![Some(7), Some(8), None]);
    // The fence stops a bullet unless it passes through small transparent objects.
    let fence = Toy { actors: vec![(7, [1000.0, 0.0, 100.0], 30.0)], fence: true, wall: None };
    let mut stopped = Projectile::spawn(&proj("Missile", &["Hitscan"], 20000.0, 0.0), None, [0.0, 0.0, 100.0], [1.0, 0.0, 0.0], Some(1), G, &t);
    assert!(matches!(&stopped.step(&fence, 0.016)[..], [ProjEvent::Hit { actor: None, .. }]));
    let mut through = Projectile::spawn(&proj("Missile", &["Hitscan", "Pass Through Small Transparent"], 20000.0, 0.0), None, [0.0, 0.0, 100.0], [1.0, 0.0, 0.0], Some(1), G, &t);
    assert!(matches!(&through.step(&fence, 0.016)[..], [ProjEvent::Hit { actor: Some(7), .. }]));
    // Range ends a missile with nothing to hit.
    let empty = Toy { actors: vec![], fence: false, wall: None };
    let mut short = Projectile::spawn(&ProjectileStats { range: 300.0, ..proj("Missile", &[], 1000.0, 0.0) }, None, [0.0, 0.0, 100.0], [1.0, 0.0, 0.0], Some(1), G, &t);
    match &fly(&mut short, &empty, 2.0)[..] {
        [ProjEvent::Expired { point }] => assert!(close(point[0], 300.0)),
        e => panic!("{e:?}"),
    }
}

#[test]
fn a_lobber_of_gravity_zero_falls_and_a_missile_of_zero_does_not() {
    // #3441: the Nuka-grenade's PROJ is a Lobber with gravity 0; it is thrown on an arc
    // like the frag grenade (gravity 1), not dead straight.
    let t = CombatTuning::default();
    let nuka = proj("Lobber", &["Explosion"], 1200.0, 0.0);
    assert_eq!(nuka.fall(), 1.0);
    let thrown = Projectile::spawn(&nuka, Some(BOOM), [0.0, 0.0, 100.0], [1.0, 0.0, 0.0], Some(1), G, &t);
    assert_eq!(thrown.gravity, G);
    assert!(fnv_combat::vats::throw_reach(nuka.speed, nuka.fall() * G).is_finite());
    // The field is still a multiplier where it is set.
    assert_eq!(proj("Lobber", &[], 1200.0, 1.5).fall(), 1.5);
    // A missile of 0 keeps flying straight.
    let missile = proj("Missile", &[], 3000.0, 0.0);
    assert_eq!(missile.fall(), 0.0);
    assert_eq!(Projectile::spawn(&missile, None, [0.0; 3], [1.0, 0.0, 0.0], Some(1), G, &t).gravity, 0.0);
}

#[test]
fn lobbers_timer_proximity_detonation() {
    let t = CombatTuning::default();
    let empty = Toy { actors: vec![], fence: false, wall: None };
    // Impact trigger: explodes where it lands.
    let mut g = Projectile::spawn(&proj("Lobber", &["Explosion"], 600.0, 1.0), Some(BOOM), [0.0, 0.0, 100.0], [1.0, 0.0, 1.0], Some(1), G, &t);
    assert!(fly(&mut g, &empty, 5.0).iter().any(|e| matches!(e, ProjEvent::Explode { .. })));
    // Timer: bounces, comes to rest, explodes at 3 s.
    let timed = ProjectileStats { alt_trigger_timer: 3.0, ..proj("Lobber", &["Explosion", "Alt. Trigger"], 600.0, 1.0) };
    let mut g = Projectile::spawn(&timed, Some(BOOM), [0.0, 0.0, 100.0], [1.0, 0.0, 1.0], Some(1), G, &t);
    let ev = fly(&mut g, &empty, 2.9);
    assert!(ev.is_empty() && g.alive);
    assert!(matches!(&fly(&mut g, &empty, 0.2)[..], [ProjEvent::Explode { normal: None, .. }]));
    // Proximity mine: it does not trigger itself, even with an actor beside it; it comes
    // to rest and leaves flight, for the engine to place as a mine.
    let mine = ProjectileStats { alt_trigger_timer: 1.0, alt_trigger_proximity: 100.0, ..proj("Lobber", &["Explosion", "Alt. Trigger"], 0.0, 1.0) };
    let mut m = Projectile::spawn(&mine, Some(BOOM), [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], Some(1), G, &t);
    let walker = Toy { actors: vec![(7, [50.0, 0.0, 30.0], 30.0)], fence: false, wall: None };
    match &fly(&mut m, &walker, 3.0)[..] {
        [ProjEvent::Settled { point, normal }] => assert!(point[2] < 1.0 && normal[2] > 0.9, "{point:?} {normal:?}"),
        e => panic!("{e:?}"),
    }
    assert!(!m.alive);
    // C-4 (Explosion and Detonates, no Alt. Trigger, as WeapNVC4PlasticExplosive's
    // projectile): set off in flight it explodes; thrown at the floor it comes to rest and
    // leaves flight, for the engine to place as a charge that waits for detonation (#1986).
    let c4 = proj("Lobber", &["Explosion", "Detonates"], 400.0, 1.0);
    let mut c = Projectile::spawn(&c4, Some(BOOM), [0.0, 0.0, 60.0], [1.0, 0.0, -0.5], Some(1), G, &t);
    assert!(matches!(c.clone().detonate(), Some(ProjEvent::Explode { normal: None, .. })));
    match &fly(&mut c, &empty, 100.0)[..] {
        [ProjEvent::Settled { point, normal }] => assert!(point[2] < 1.0 && normal[2] > 0.9, "{point:?} {normal:?}"),
        e => panic!("{e:?}"),
    }
    assert!(!c.alive && c.detonate().is_none());
}

/// A thrown grenade (a lobber with a fuse) rolls on after it lands, slowing, and goes off
/// on its fuse; a launcher's round (a missile) goes off where it first lands; both record
/// where and when they first touched (combat-rules.md §5, issues #1471, #1785).
#[test]
fn thrown_grenades_roll_launcher_rounds_do_not() {
    let t = CombatTuning::default();
    let floor = Toy { actors: vec![], fence: false, wall: None };
    let frag = ProjectileStats { alt_trigger_timer: 2.5, ..proj("Lobber", &["Explosion", "Alt. Trigger"], 900.0, 1.0) };
    let mut g = Projectile::spawn(&frag, Some(BOOM), [0.0, 0.0, 120.0], [1.0, 0.0, 0.0], Some(1), G, &t);
    assert!(g.rolls);
    let mut ev = fly(&mut g, &floor, 1.0);
    let (point, at) = g.first_contact.expect("it landed");
    // From 120 units up it lands after √(2 × 120 / g) s, at 900 units/s.
    let fall = (2.0 * 120.0 / G).sqrt();
    assert!((at - fall).abs() < 0.02 && (point[0] - 900.0 * fall).abs() < 15.0, "{point:?} {at}");
    assert!(g.rolling && g.alive && g.pos[2] < 2.0, "rolls on the ground: {:?}", g.pos);
    let rolled = g.pos[0];
    assert!(rolled > point[0] + 200.0, "{rolled} {point:?}");
    ev.extend(fly(&mut g, &floor, 2.0));
    match &ev[..] {
        [ProjEvent::Explode { point: at_end, .. }] => {
            // Slowing as it rolls: short of where it would be at the speed it rolled off with.
            let roll = at_end[0] - point[0];
            let undamped = t.grenade_roll_keep * 900.0 * (2.5 - fall);
            assert!(at_end[0] > rolled && roll > 0.5 * undamped && roll < 0.9 * undamped, "{at_end:?} {rolled} {point:?} {undamped}");
            assert!(at_end[2] < 2.0, "{at_end:?}");
        }
        e => panic!("{e:?}"),
    }
    // The launcher's round: no roll, it goes off where it lands.
    let round = proj("Missile", &["Explosion"], 1750.0, 1.5);
    let mut r = Projectile::spawn(&round, Some(BOOM), [0.0, 0.0, 110.0], [1.0, 0.0, 0.0], Some(1), G, &t);
    assert!(!r.rolls);
    let ev = fly(&mut r, &floor, 2.0);
    let (point, at) = r.first_contact.expect("it landed");
    let fall = (2.0 * 110.0 / (1.5 * G)).sqrt();
    assert!((at - fall).abs() < 0.02 && (point[0] - 1750.0 * fall).abs() < 20.0, "{point:?} {at}");
    assert!(matches!(&ev[..], [ProjEvent::Hit { .. }, ProjEvent::Explode { point: p, .. }] if (p[0] - point[0]).abs() < 1e-3), "{ev:?}");
    // A slow roll comes to rest before the fuse, as the original's taps did (o32_throw):
    // 300 units/s keeps 180, which 140 a second stops in about 1.3 s, some 115 units on.
    let tap = ProjectileStats { alt_trigger_timer: 2.5, ..proj("Lobber", &["Explosion", "Alt. Trigger"], 300.0, 1.0) };
    let mut slow = Projectile::spawn(&tap, Some(BOOM), [0.0, 0.0, 20.0], [1.0, 0.0, 0.0], Some(1), G, &t);
    fly(&mut slow, &floor, 2.2);
    let (landed_at, _) = slow.first_contact.expect("it landed");
    let rolled = slow.pos[0] - landed_at[0];
    assert!(slow.landed && (80.0..160.0).contains(&rolled), "{rolled} {:?}", slow.pos);
    // A frag mine is no thrown grenade: it settles where it lands.
    let mine = ProjectileStats { alt_trigger_timer: 1.0, alt_trigger_proximity: 100.0, ..proj("Lobber", &["Explosion", "Alt. Trigger"], 0.0, 1.0) };
    assert!(!Projectile::spawn(&mine, Some(BOOM), [0.0; 3], [1.0, 0.0, 0.0], Some(1), G, &t).rolls);
}

/// A pond on the toy floor: water 50 deep (its surface at z = 50) from x = 300 on.
struct Pond(Toy);

impl CombatWorld for Pond {
    fn ray(&self, from: V3, to: V3, ignore: &[ActorId]) -> Option<WorldHit> {
        self.0.ray(from, to, ignore)
    }

    fn actors_near(&self, point: V3, radius: f32) -> Vec<(ActorId, V3)> {
        self.0.actors_near(point, radius)
    }

    fn water_surface(&self, point: V3) -> Option<f32> {
        (point[0] >= 300.0).then_some(50.0)
    }
}

fn splashes(ev: &[ProjEvent]) -> Vec<V3> {
    ev.iter().filter_map(|e| if let ProjEvent::Splash { point, .. } = e { Some(*point) } else { None }).collect()
}

#[test]
fn shots_splash_into_water_and_strike_what_is_under_it() {
    let t = CombatTuning::default();
    // From the bank (no water under the shooter) down at an actor under the surface: a
    // splash where the line meets the surface (5/8 of the way, x = 625), then the hit.
    let under = Pond(Toy { actors: vec![(7, [1000.0, 0.0, 20.0], 15.0)], fence: false, wall: None });
    let dir = vec::normalize([1000.0, 0.0, -80.0]);
    let mut p = Projectile::spawn(&proj("Missile", &["Hitscan"], 20000.0, 0.0), None, [0.0, 0.0, 100.0], dir, Some(1), G, &t);
    match &p.step(&under, 1.0 / 60.0)[..] {
        [ProjEvent::Splash { point, .. }, ProjEvent::Hit { actor: Some(7), .. }] => assert!(close(point[2], 50.0) && (point[0] - 625.0).abs() < 1.0, "{point:?}"),
        e => panic!("{e:?}"),
    }
    // A wall on the bank stops it before the water: no splash.
    let walled = Pond(Toy { wall: Some(400.0), ..under.0 });
    let mut p = Projectile::spawn(&proj("Missile", &["Hitscan"], 20000.0, 0.0), None, [0.0, 0.0, 100.0], dir, Some(1), G, &t);
    assert!(matches!(&p.step(&walled, 1.0 / 60.0)[..], [ProjEvent::Hit { actor: None, .. }]));
    // Level over the water: none either.
    let empty = Pond(Toy { actors: vec![], fence: false, wall: None });
    let mut p = Projectile::spawn(&ProjectileStats { range: 2000.0, ..proj("Missile", &["Hitscan"], 20000.0, 0.0) }, None, [0.0, 0.0, 100.0], [1.0, 0.0, 0.0], Some(1), G, &t);
    assert!(splashes(&p.step(&empty, 1.0 / 60.0)).is_empty());
    // A flying missile falls into it: one splash on the surface, then the bed.
    let mut m = Projectile::spawn(&proj("Missile", &[], 1000.0, 1.0), None, [0.0, 0.0, 100.0], [1.0, 0.0, 0.0], Some(1), G, &t);
    let mut ev = Vec::new();
    for _ in 0..120 {
        ev.extend(m.step(&empty, 1.0 / 60.0));
    }
    let s = splashes(&ev);
    assert!(s.len() == 1 && close(s[0][2], 50.0) && s[0][0] > 300.0, "{ev:?}");
    assert!(matches!(ev.last(), Some(ProjEvent::Hit { actor: None, point, .. }) if point[2].abs() < 1e-3), "{ev:?}");
    // A grenade bouncing on the bed splashes once, however often it comes up.
    let timed = ProjectileStats { alt_trigger_timer: 3.0, ..proj("Lobber", &["Explosion", "Alt. Trigger"], 600.0, 1.0) };
    let mut g = Projectile::spawn(&timed, Some(BOOM), [0.0, 0.0, 100.0], [1.0, 0.0, 0.5], Some(1), G, &t);
    let mut ev = Vec::new();
    for _ in 0..240 {
        ev.extend(g.step(&empty, 1.0 / 60.0));
    }
    assert_eq!(splashes(&ev).len(), 1, "{ev:?}");
}

#[test]
fn ordinary_ground_melee_keeps_its_fixed_body_margin() {
    // Synthetic low body and short weapon [design, #2535]. No eye-height ray
    // reaches it; the default ground fallback ends at reach + 24 exactly.
    let t = CombatTuning::default();
    let reach = 53.5 + 22.0;
    for (distance, expected) in [(reach + 24.0 - 0.01, true), (reach + 24.0 + 0.01, false)] {
        for forward in [[1.0, 0.0, 0.0], vec::normalize([1.0, 0.0, -1.0])] {
            let world = Toy { actors: vec![(7, [distance, 0.0, 15.0], 15.0)], fence: false, wall: None };
            let hits = swing_reaching(&world, &t, [0.0, 0.0, 112.0], 0.0, forward, reach, 1);
            assert_eq!(hits.iter().any(|h| h.0 == 7), expected);
            let blocked = Toy { wall: Some(40.0), ..world };
            assert!(swing_reaching(&blocked, &t, [0.0, 0.0, 112.0], 0.0, forward, reach, 1).is_empty());
        }
    }
}

#[test]
fn melee_swing_and_blast() {
    let t = CombatTuning::default();
    let world = Toy { actors: vec![(7, [80.0, 20.0, 100.0], 25.0), (8, [-80.0, 0.0, 100.0], 25.0), (9, [300.0, 0.0, 100.0], 25.0)], fence: false, wall: None };
    let hits = swing(&world, &t, [0.0, 0.0, 100.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], 120.0, 1);
    assert_eq!(hits.iter().map(|h| h.0).collect::<Vec<_>>(), vec![7]);
    // A short creature at the attacker's feet (a gecko): under the fan of rays at eye
    // height, and the drop from the eye would use up the reach; reached across the
    // ground it is struck. One behind is not, nor one beyond the reach.
    let low = Toy { actors: vec![(7, [70.0, 10.0, 15.0], 15.0), (8, [-70.0, 0.0, 15.0], 15.0), (9, [200.0, 0.0, 15.0], 15.0)], fence: false, wall: None };
    let eye = [0.0, 0.0, 112.0];
    assert!(swing(&low, &t, eye, [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], 120.0, 1).is_empty());
    let hits = swing_reaching(&low, &t, eye, 0.0, [1.0, 0.0, 0.0], 120.0, 1);
    assert_eq!(hits.iter().map(|h| h.0).collect::<Vec<_>>(), vec![7]);
    // A tall one in front is still found by the fan, once.
    let both = Toy { actors: vec![(7, [70.0, 0.0, 15.0], 15.0), (8, [90.0, 30.0, 100.0], 25.0)], fence: false, wall: None };
    let mut ids: Vec<_> = swing_reaching(&both, &t, eye, 0.0, [1.0, 0.0, 0.0], 120.0, 1).iter().map(|h| h.0).collect();
    ids.sort();
    assert_eq!(ids, vec![7, 8]);
    let e =ExplosionStats { damage: 100.0, radius: 400.0, force: 300.0, ..ExplosionStats::default() };
    let open = Toy { actors: vec![(7, [200.0, 0.0, 30.0], 30.0)], fence: false, wall: None };
    let b = blast(&open, &t, &e, [0.0, 0.0, 30.0], None);
    assert_eq!(b.len(), 1);
    assert!(close(b[0].damage, 62.5) && close(b[0].push[0], 1.0));
    // A wall between the blast and the actor shields it, unless the EXPL ignores LOS.
    let walled = Toy { actors: vec![(7, [200.0, 0.0, 30.0], 30.0)], fence: false, wall: Some(100.0) };
    assert!(blast(&walled, &t, &e, [0.0, 0.0, 30.0], None).is_empty());
    let ignore = ExplosionStats { flags: vec!["Ignore LOS Check".into()], ..e.clone() };
    assert_eq!(blast(&walled, &t, &ignore, [0.0, 0.0, 30.0], None).len(), 1);
    // A pack in a line: the ones behind are not shielded by the bodies in front.
    let pack = Toy { actors: vec![(7, [60.0, 0.0, 30.0], 30.0), (8, [130.0, 0.0, 30.0], 30.0), (9, [200.0, 0.0, 30.0], 30.0)], fence: false, wall: None };
    let mut ids: Vec<_> = blast(&pack, &t, &e, [0.0, 0.0, 30.0], None).iter().map(|b| b.actor).collect();
    ids.sort();
    assert_eq!(ids, vec![7, 8, 9]);
    // A wall still shields the pack behind it.
    let behind = Toy { wall: Some(100.0), ..pack };
    assert_eq!(blast(&behind, &t, &e, [0.0, 0.0, 30.0], None).iter().map(|b| b.actor).collect::<Vec<_>>(), vec![7]);
}

#[test]
fn melee_reach_power_and_block() {
    let r = Rules::defaults();
    let t = CombatTuning::default();
    let knife = WeaponStats { anim_type: Some("Melee (1 Hand)".into()), reach: 0.5, ..gun(10) };
    let fists = WeaponStats { anim_type: Some("Hand to Hand".into()), reach: 0.0, ..gun(0) };
    assert!(close(damage::melee_reach(&r, &knife, 1.0), 0.5 * r.combat_distance));
    // A reach of 0 (the Fists) counts as 1.
    assert!(close(damage::melee_reach(&r, &fists, 1.0), r.combat_distance));
    // V.A.T.S. (issue #2656): the swing's reach doubled, at most 300 [documented: GECK wiki
    // "VATS Settings", fVATSMeleeReachMult 2, fVATSMeleeMaxDistance 300].
    assert!(close(r.vats_melee_reach_mult, 2.0) && close(r.vats_melee_max_distance, 300.0));
    assert!(close(damage::vats_melee_reach(&r, &knife, 1.0), 107.0));
    assert!(close(damage::vats_melee_reach(&r, &fists, 1.0), 214.0));
    let pipe = WeaponStats { anim_type: Some("Melee (2 Hand)".into()), reach: 1.25, ..gun(10) };
    assert!(close(damage::vats_melee_reach(&r, &pipe, 1.0), 267.5));
    let long = WeaponStats { reach: 1.5, ..pipe.clone() };
    assert!(close(damage::vats_melee_reach(&r, &long, 1.0), 300.0), "capped");
    let mut far = Rules::defaults();
    (far.vats_melee_reach_mult, far.vats_melee_max_distance) = (6.0, 1000.0);
    assert!(close(damage::vats_melee_reach(&far, &fists, 1.0), 642.0), "the settings are read");
    // A power attack multiplies the base before armour, by its direction's setting.
    let a = Attacker { strength: 10.0, ..skilled() };
    let power = fnv_combat::melee::PowerDir::Stand.damage(&r);
    assert!(close(power, 3.0) && close(fnv_combat::melee::PowerDir::Left.damage(&r), 2.0));
    let h = HitInput { weapon: &knife, projectiles: 1, condition: 1.0, ammo_effects: &[], part: HitPart::default(), power };
    let d = damage::hit(&r, &t, &h, &a, &Target::default(), &NoPerks, &NoPerks, 0.99);
    assert!(close(d.dealt, (10.0 + 5.0) * power), "{d:?}");
    // Fists do the UnarmedDamage actor value; it is no miss.
    let h = HitInput { weapon: &fists, projectiles: 1, condition: 1.0, ammo_effects: &[], part: HitPart::default(), power: 1.0 };
    let d = damage::hit(&r, &t, &h, &Attacker { unarmed_skill: 40.0, ..Attacker::default() }, &Target::default(), &NoPerks, &NoPerks, 0.99);
    assert!(!d.miss && close(d.dealt, 0.5 + 0.05 * 40.0));
    assert!(close(damage::blocked(&t, 10.0), 10.0 * (1.0 - t.block)));
    // A power attack breaks through a block: all of it is dealt and the guard breaks.
    assert_eq!(damage::against_block(&t, 10.0, true), (10.0, true));
    let (left, broken) = damage::against_block(&t, 10.0, false);
    assert!(close(left, damage::blocked(&t, 10.0)) && !broken);
}

/// Where a shot leaves and where it is seen to leave (docs/design/combat.md §5.1).
#[test]
fn shots_are_aimed_from_the_eye_and_seen_from_the_muzzle() {
    let t = CombatTuning::default();
    let eye = [0.0, 0.0, 100.0];
    let aim = [1.0, 0.0, 0.0];
    let muzzle = [20.0, 10.0, 60.0];
    let wall = Toy { actors: vec![], fence: false, wall: Some(800.0) };
    // Without a muzzle the shot leaves the eye and is drawn where it flies.
    let l = shot::leaving(&wall, eye, aim, None, 5000.0, Some(1), true);
    assert_eq!((l.origin, l.aim, l.seen, l.covered), (eye, aim, Seen::ON_PATH, false));
    assert!(close(l.aim_distance, 800.0) && l.aim_point == [800.0, 0.0, 100.0]);
    // With one it still leaves the eye along the view; it is drawn from the muzzle and
    // is on its path by the aim point.
    let l = shot::leaving(&wall, eye, aim, Some(muzzle), 5000.0, Some(1), true);
    assert_eq!((l.origin, l.aim, l.covered), (eye, aim, false));
    assert_eq!(l.seen.at(eye, 0.0), muzzle);
    assert_eq!(l.seen.at([400.0, 0.0, 100.0], 400.0), [410.0, 5.0, 80.0]);
    assert_eq!(l.seen.at(l.aim_point, 800.0), l.aim_point);
    assert_eq!(l.seen.at([900.0, 0.0, 100.0], 900.0), [900.0, 0.0, 100.0]);
    // Nothing in the way of the view: the offset closes over the reach.
    let open = Toy { actors: vec![], fence: false, wall: None };
    let l = shot::leaving(&open, eye, aim, Some(muzzle), 5000.0, Some(1), true);
    assert!(close(l.aim_distance, 5000.0) && close(l.seen.converge, 5000.0) && !l.covered);
    // Something the muzzle's ray meets and the view passes over, but far from the
    // barrel (the ray goes through z = 80 at x = 400): no cover.
    let far = Toy { actors: vec![(8, [400.0, 5.0, 70.0], 15.0)], fence: false, wall: Some(800.0) };
    let l = shot::leaving(&far, eye, aim, Some(muzzle), 5000.0, Some(1), true);
    assert!(!l.covered && l.origin == eye && l.seen.offset == vec::sub(muzzle, eye));
    // The same right in front of the barrel (the ray goes through (100, 9, 64)): in
    // third person the projectiles leave the muzzle for the aim point and strike it;
    // in first person they do not.
    let cover = Toy { actors: vec![(8, [100.0, 9.0, 60.0], 10.0)], fence: false, wall: Some(800.0) };
    let l = shot::leaving(&cover, eye, aim, Some(muzzle), 5000.0, Some(1), false);
    assert!(!l.covered && l.origin == eye);
    let l = shot::leaving(&cover, eye, aim, Some(muzzle), 5000.0, Some(1), true);
    assert!(l.covered && l.origin == muzzle && l.seen == Seen::ON_PATH);
    assert!(close(vec::len(vec::sub(vec::add(muzzle, vec::scale(l.aim, vec::len(vec::sub(l.aim_point, muzzle)))), l.aim_point)), 0.0));
    let mut p = Projectile::spawn(&proj("Missile", &["Hitscan"], 20000.0, 0.0), None, l.origin, l.aim, Some(1), G, &t);
    assert!(matches!(&p.step(&cover, 1.0 / 60.0)[..], [ProjEvent::Hit { actor: Some(8), .. }]));
    // The actor aimed at is no cover, though the muzzle's ray meets it sooner.
    let target = Toy { actors: vec![(7, [400.0, 0.0, 100.0], 40.0)], fence: false, wall: None };
    let l = shot::leaving(&target, eye, aim, Some([20.0, 10.0, 60.0]), 5000.0, Some(1), true);
    assert!(!l.covered && l.origin == eye && close(l.aim_distance, 360.0));
    // A barrel pushed past the aim point: drawn from the eye.
    let l = shot::leaving(&wall, eye, aim, Some([820.0, 10.0, 60.0]), 5000.0, Some(1), true);
    assert_eq!((l.origin, l.seen, l.covered), (eye, Seen::ON_PATH, false));
}

/// The combat perk entry points on one hit (docs/spec-notes/perk-entry-points.md §2),
/// with the numbers of FalloutNV.esm's perks: Bloody Mess's weapon damage × 1.05, Laser
/// Commander's crit chance + 10, Better Criticals' crit damage × 1.5, Dream Crusher's
/// enemy crit chance × 0.5, Toughness's own DT + 3 and Shotgun Surgeon's 10 DT ignored
/// (Set 10 from 0).
#[test]
fn perk_entry_points_change_the_hit() {
    use damage::{entry, EntryOps, PerkHooks, PerkOp};
    let ops = |v: &[(u8, PerkOp)]| EntryOps(v.iter().map(|(c, o)| (*c, *o, "perk".to_string())).collect());
    let w = gun(20);
    let h = HitInput { weapon: &w, projectiles: 1, condition: 1.0, ammo_effects: &[], part: HitPart::default(), power: 1.0 };
    let r = Rules::defaults();
    let t = CombatTuning::default();
    let run = |a: &EntryOps, d: &EntryOps, dt: f32, roll: f32| damage::hit(&r, &t, &h, &skilled(), &Target { dt, ..Target::default() }, a, d, roll);
    let none = EntryOps::default();

    // Weapon damage: before DT and not on the critical damage.
    let bloody = ops(&[(entry::CALCULATE_WEAPON_DAMAGE, PerkOp::Mul(1.05))]);
    let d = run(&bloody, &none, 0.0, 0.99);
    assert!(close(d.base, 20.0) && close(d.modified, 21.0) && close(d.dealt, 21.0), "{d:?}");
    let c = run(&bloody, &none, 0.0, 0.0);
    assert!(c.critical && close(c.crit_bonus, 10.0) && close(c.dealt, 31.0), "{c:?}");

    // Crit chance: Luck 5 gives 5 %, + 10.
    let base = run(&none, &none, 0.0, 0.99).crit_chance;
    let laser = ops(&[(entry::CALCULATE_MY_CRITICAL_HIT_CHANCE, PerkOp::Add(10.0))]);
    assert!(close(run(&laser, &none, 0.0, 0.99).crit_chance, base + 10.0));
    // A roll of 0.12 crits only with it.
    assert!(!run(&none, &none, 0.0, 0.12).critical && run(&laser, &none, 0.0, 0.12).critical);

    // Crit damage.
    let better = ops(&[(entry::CALCULATE_MY_CRITICAL_HIT_DAMAGE, PerkOp::Mul(1.5))]);
    assert!(close(run(&better, &none, 0.0, 0.0).crit_bonus, 15.0));

    // The target's perk halves the attacker's crit chance.
    let dream = ops(&[(entry::MODIFY_ENEMY_CRITICAL_HIT_CHANCE, PerkOp::Mul(0.5))]);
    assert!(close(run(&none, &dream, 0.0, 0.99).crit_chance, base * 0.5));

    // DT: the defender's own + 3, the attacker's 10 ignored, never below 0.
    let tough = ops(&[(entry::MODIFY_DAMAGE_THRESHOLD_DEFENDER, PerkOp::Add(3.0))]);
    assert!(close(run(&none, &tough, 5.0, 0.99).dt, 8.0) && close(run(&none, &tough, 5.0, 0.99).dealt, 12.0));
    let surgeon = ops(&[(entry::MODIFY_DAMAGE_THRESHOLD_ATTACKER, PerkOp::Set(10.0))]);
    assert!(close(run(&surgeon, &tough, 12.0, 0.99).dt, 5.0));
    assert!(close(run(&surgeon, &none, 4.0, 0.99).dt, 0.0));

    // Entries of other entry points leave the hit alone; several of one apply in order.
    let spread = ops(&[(entry::CALCULATE_GUN_SPREAD, PerkOp::Set(0.0))]);
    assert!(close(run(&spread, &none, 0.0, 0.99).dealt, 20.0));
    let two = ops(&[(entry::CALCULATE_WEAPON_DAMAGE, PerkOp::Add(2.0)), (entry::CALCULATE_WEAPON_DAMAGE, PerkOp::Mul(2.0))]);
    assert!(close(two.weapon_damage(20.0), 44.0));
    assert!(close(PerkOp::Abs.apply(-3.0), 3.0) && close(PerkOp::NegAbs.apply(3.0), -3.0));
}

/// Reload speed from Agility: 0.5 + 0.1 × Agility times the clip's speed, so a reload
/// takes 5/3 as long at Agility 1 and 2/3 as long at 10 [documented: Fallout Wiki,
/// Agility (Fallout: New Vegas)]; a round-by-round reload's parts all scale.
#[test]
fn reload_time_follows_agility() {
    use fnv_combat::attack::reload_rate;
    assert!(close(reload_rate(5.0), 1.0) && close(reload_rate(1.0), 0.6) && close(reload_rate(10.0), 1.5));
    assert!(close(reload_rate(0.0), 0.6) && close(reload_rate(14.0), 1.5), "Agility read within 1 to 10");
    let w = gun(10);
    let t = CombatTuning::default();
    let clips = ClipTiming { reload_len: Some(2.0), reload_hit: Some(0.5), reload_open_len: Some(0.6), ..ClipTiming::default() };
    let base = Timing::of(&w, &clips, &t);
    let slow = base.with_reload_rate(reload_rate(1.0));
    assert!(close(slow.reload, 2.0 / 0.6), "{slow:?}");
    let fast = base.with_reload_rate(reload_rate(10.0) * 1.25);
    assert!(close(fast.reload, 2.0 / 1.875), "Rapid Reload's 1.25 on top: {fast:?}");
    let r = fast.rounds.expect("round by round");
    assert!(close(r.open, 0.6 / 1.875) && close(r.round, 0.5 / 1.875) && close(r.close, 1.5 / 1.875));
    // Nothing else changes; a rate of 0 or below is read as 1.
    assert!(close(fast.interval, base.interval) && close(fast.fire_delay, base.fire_delay));
    assert!(close(base.with_reload_rate(0.0).reload, 2.0));
}

/// The Modify Attack Speed perks: the shot cycle runs `rate` times as fast; the reload
/// does not change (perk-entry-points.md §10).
#[test]
fn attack_rate_shortens_the_cycle_not_the_reload() {
    let base = Timing { interval: 0.6, fire_delay: 0.2, reload: 2.0, spin_up: 0.0, rounds: None, attach: Some(0.4) };
    let fast = base.with_attack_rate(1.2);
    assert!(close(fast.interval, 0.5) && close(fast.fire_delay, 0.2 / 1.2) && close(fast.attach.unwrap(), 0.4 / 1.2), "{fast:?}");
    assert!(close(fast.reload, 2.0));
    let slow = base.with_attack_rate(0.8);
    assert!(close(slow.interval, 0.75));
    assert!(close(base.with_attack_rate(0.0).interval, 0.6), "0 is read as 1");
}

/// Companion Nerve: a teammate's hits do 5 % more per point of the player's Charisma
/// and its DT and DR go up by the same share [documented: Fallout Wiki, Charisma (Fallout:
/// New Vegas), "+5% to their damage and armor"].
#[test]
fn companion_nerve_raises_damage_and_armour() {
    let w = gun(20);
    let plain = hit(&w, 1, &skilled(), &Target::default(), &[], HitPart::default(), 0.99);
    let nerve = hit(&w, 1, &Attacker { nerve: 0.25, ..skilled() }, &Target::default(), &[], HitPart::default(), 0.99);
    assert!(close(plain.dealt, 20.0) && close(nerve.dealt, 25.0), "{nerve:?}");
    // DT 8 at Charisma 5 (+25 %) is 10; DR 10 % is 12.5 %.
    let t = Target { dt: 8.0, dr: 10.0, nerve: 0.25, ..Target::default() };
    let d = hit(&w, 1, &skilled(), &t, &[], HitPart::default(), 0.99);
    assert!(close(d.dt, 10.0) && close(d.dr, 12.5) && close(d.dealt, 10.0 * 0.875), "{d:?}");
}

#[test]
fn the_radius_buff_keeps_the_inner_part_whole() {
    // bExplosionRadiusBuff (docs/design/tweaks.md): full damage to 30% of the radius,
    // 20% at its edge.
    let t = CombatTuning { explosion_inner: 0.3, explosion_inner_edge: 0.2, ..CombatTuning::default() };
    assert_eq!(fnv_combat::damage::explosion_damage(&t, 100.0, 100.0, 20.0), 100.0);
    assert!((fnv_combat::damage::explosion_damage(&t, 100.0, 100.0, 100.0) - 20.0).abs() < 1e-3);
    assert!((fnv_combat::damage::explosion_damage(&CombatTuning::default(), 100.0, 100.0, 100.0) - 25.0).abs() < 1e-3);
}

/// #3023: a weapon whose Ammo Use is 0 (the Laser Detonator: automatic, a clip of one, Fire
/// Rate 10) spends no round a shot: held, it fires at its rate with no reload between
/// shots, from an empty magazine and with nothing carried.
#[test]
fn ammo_use_zero_spends_no_round_and_never_reloads() {
    let t = CombatTuning::default();
    let w = WeaponStats { clip_size: 1, ammo_use: 0, automatic: true, attack_shots_per_sec: 10.0, ..gun(20) };
    let timing = Timing { interval: 0.1, fire_delay: 0.0, reload: 2.0, spin_up: 0.0, rounds: None, attach: None };
    for magazine in [1, 0] {
        let mut c = Cycle::new(&w, timing, false, magazine);
        let ev = run(&mut c, &t, 200, 0.01, |_| true, 0);
        assert!(!ev.iter().any(|e| matches!(e, Event::ReloadStarted | Event::DryFire)), "{ev:?}");
        assert!(ev.iter().all(|e| !matches!(e, Event::Fired { rounds } if *rounds != 0)), "{ev:?}");
        let n = shots(&ev);
        assert!((19..=21).contains(&n), "about 10 a second for 2 s from a magazine of {magazine}: {n}");
        assert_eq!(c.magazine, magazine);
    }
}
