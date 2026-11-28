//! Co-op: the other players as this game sees them, and aiming at a downed one to revive it.
//!
//! - **The mates.** `nvforever`'s `coop.rs` fills [`SocialMates`] every frame from what the
//!   link heard: each other player's id, name, the reference its stand-in has here, whether
//!   it is in this player's cell or worldspace, its feet, facing, whether it is down (and
//!   where its body lies) and whether its connection is lost. Outside co-op the list is
//!   empty and nothing here does anything.
//! - **Revive targeting.** While another player is down within [`REVIVE_REACH`] along the
//!   crosshair, its body (its pelvis, or its feet while that is not known) passing within
//!   [`DOWN_RADIUS`] of the ray, the crosshair's target is that player with
//!   [`Action::Revive`](crate::activate::outcome::Action::Revive): the game's own rollover,
//!   "Revive" over the name. A wall nearer than the body keeps it [design].
//! - **The revive kit** ([`ReviveKit`]): what a revive would use, a Stimpak carried (an
//!   ingestible whose equipment type is Stimpak, [`fnv_game::data::IngestibleDef::STIMPAK`]),
//!   or nothing; the rollover greys out with nothing [design: a revive uses a Stimpak, as
//!   the game's own healing of a companion does].
//!
//! The social layer builds its own targeting of standing players on the same list.

use bevy::prelude::*;
use fnv_sim::FormId;

use crate::activate::outcome::Action;
use crate::player::Player;
use crate::PlaySet;

/// Another player, as this game draws it ([`SocialMates`]).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mate {
    pub id: u8,
    pub name: String,
    /// Its stand-in's reference here (its actor's on the host).
    pub reference: u32,
    /// In this player's cell or worldspace.
    pub here: bool,
    pub feet: [f32; 3],
    pub yaw: f32,
    pub down: bool,
    /// Down: where its body lies here (its stand-in's pelvis, game space), which is where it
    /// is revived; `None` while not known (its feet stand in).
    pub body: Option<[f32; 3]>,
    /// Its connection is lost for now.
    pub lost: bool,
}

/// The other players in this co-op game (empty outside co-op).
#[derive(Resource, Default, Debug, Clone, PartialEq)]
pub struct SocialMates {
    /// This player's id in the lobby (None: no lobby).
    pub me: Option<u8>,
    /// This player is indoors.
    pub interior: bool,
    pub mates: Vec<Mate>,
}

/// How near the crosshair's ray must pass a downed body's pelvis to aim at it (game
/// units) [design: about the body's half length, lying].
pub const DOWN_RADIUS: f32 = 45.0;

/// How far a downed body can be revived from, along the crosshair (game units) [design:
/// a little more than the reach of activation, as the body lies lower].
pub const REVIVE_REACH: f32 = 240.0;

/// Where the crosshair's ray from `eye` along `dir` passes a downed body's point `at` (its
/// pelvis) within [`DOWN_RADIUS`]: the distance along the ray, within `reach`.
pub fn ray_meets_lying(eye: [f32; 3], dir: [f32; 3], at: [f32; 3], reach: f32) -> Option<f32> {
    let len = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]).sqrt().max(1e-6);
    let d = [dir[0] / len, dir[1] / len, dir[2] / len];
    let to = [at[0] - eye[0], at[1] - eye[1], at[2] - eye[2]];
    let t = (to[0] * d[0] + to[1] * d[1] + to[2] * d[2]).max(0.0);
    let off = [to[0] - d[0] * t, to[1] - d[1] * t, to[2] - d[2] * t];
    let off = (off[0] * off[0] + off[1] * off[1] + off[2] * off[2]).sqrt();
    (off <= DOWN_RADIUS && t <= reach).then_some(t)
}

/// What this player revives a downed player with: the item and its name; `None`: nothing.
/// Kept while another player is down; read by the crosshair's "Revive" rollover and by the
/// revive (`nvforever`'s `coop.rs`).
#[derive(Resource, Default, Debug, Clone, PartialEq)]
pub struct ReviveKit(pub Option<(FormId, String)>);

/// The Stimpak among `carried` (item, count): an ingestible whose equipment type is
/// Stimpak.
pub fn revive_item(data: &fnv_game::data::GameData, carried: impl IntoIterator<Item = (FormId, i32)>) -> Option<FormId> {
    carried.into_iter().filter(|(_, n)| *n > 0).map(|(f, _)| f).filter(|f| data.ingestibles.get(f).is_some_and(|i| i.equip_type == fnv_game::data::IngestibleDef::STIMPAK)).min_by_key(|f| f.0)
}

fn revive_kit(script: Res<crate::script::ScriptState>, sim: Option<Res<crate::sim::SimState>>, mates: Res<SocialMates>, mut kit: ResMut<ReviveKit>) {
    if !mates.mates.iter().any(|m| m.down) {
        if kit.0.is_some() {
            kit.0 = None;
        }
        return;
    }
    let (Some(rt), Some(sim)) = (script.rt.as_ref(), sim.as_deref()) else { return };
    let carried: Vec<(FormId, i32)> = sim.world.peek(FormId::PLAYER_REF).and_then(|s| s.inventory.as_ref()).map(|i| i.iter().collect()).unwrap_or_default();
    let named = revive_item(rt.game.data, carried).map(|f| (f, rt.game.data.full_names.get(&f).cloned().unwrap_or_else(|| rt.game.data.name(f))));
    if kit.0 != named {
        info!("co-op: a revive would use {}", named.as_ref().map_or("nothing (no Stimpak)", |(_, n)| n.as_str()));
        kit.0 = named;
    }
}

/// The crosshair on a downed player (module notes): it takes the world's target.
fn target(player: Option<Res<Player>>, control: Res<crate::control::PlayerControl>, mates: Res<SocialMates>, kit: Res<ReviveKit>, mut phys: Option<ResMut<crate::phys::PhysState>>, mut found: ResMut<crate::activate::Target>) {
    let Some(player) = player else { return };
    if mates.me.is_none() || !mates.mates.iter().any(|m| m.down) {
        return;
    }
    let (eye, dir) = crate::control::Crosshair::ray(control.crosshair.as_ref(), player.eye(), player.look_dir());
    let wall = phys.as_deref_mut().and_then(|p| {
        let mut f = p.player.as_ref().map_or_else(fnv_phys::Filter::default, |c| fnv_phys::Filter::excluding(c.key()));
        f.exclude_layers.insert(fnv_phys::layers::TRIGGER);
        p.world.queries().ray(eye, dir, REVIVE_REACH, &f).filter(|h| !matches!(h.owner, fnv_phys::Owner::Actor { .. })).map(|h| h.distance)
    });
    let down = mates
        .mates
        .iter()
        .filter(|m| m.here && !m.lost && m.down)
        .filter_map(|m| {
            let at = m.body.unwrap_or([m.feet[0], m.feet[1], m.feet[2] + 15.0]);
            ray_meets_lying(eye, dir, at, REVIVE_REACH).filter(|d| wall.is_none_or(|w| *d <= w + DOWN_RADIUS)).map(|d| (d, m))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0));
    if let Some((d, m)) = down {
        let t = crate::activate::Targeted {
            form_id: m.reference,
            name: m.name.clone(),
            action: Action::Revive { with: kit.0.as_ref().map(|(_, n)| n.clone()) },
            distance: d,
            source: crate::activate::pick::Source::Collision,
            crime: false,
            empty: false,
            through: None,
        };
        if found.0.as_ref() != Some(&t) {
            found.0 = Some(t);
        }
    }
}

pub struct CoopMatesPlugin;

impl Plugin for CoopMatesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SocialMates>()
            .init_resource::<ReviveKit>()
            .add_systems(Update, revive_kit.in_set(PlaySet::Activation).before(target))
            .add_systems(Update, target.in_set(PlaySet::Activation).after(crate::activate::find_target).before(crate::activate::activate));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ray_meets_a_body_lying_across_it_within_reach_only() {
        let eye = [0.0, 0.0, 100.0];
        // Straight ahead and down onto a body 150 units away.
        let at = [150.0, 0.0, 15.0];
        let dir = [at[0] - eye[0], at[1] - eye[1], at[2] - eye[2]];
        let d = ray_meets_lying(eye, dir, at, REVIVE_REACH).expect("aimed at the body");
        assert!((d - (150.0f32.powi(2) + 85.0f32.powi(2)).sqrt()).abs() < 0.5);
        // Off to the side by more than the radius.
        assert_eq!(ray_meets_lying(eye, dir, [150.0, DOWN_RADIUS + 30.0, 15.0], REVIVE_REACH), None);
        // Beyond the reach.
        assert_eq!(ray_meets_lying(eye, [1.0, 0.0, 0.0], [REVIVE_REACH + 50.0, 0.0, 100.0], REVIVE_REACH), None);
        // Behind the eye.
        assert_eq!(ray_meets_lying(eye, [1.0, 0.0, 0.0], [-100.0, 0.0, 100.0], REVIVE_REACH), None);
    }
}
