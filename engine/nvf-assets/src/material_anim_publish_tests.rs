use super::*;
use fnv_nif::anim::{Channel, Key, KeyKind, Keys, ValueTrack};

#[test]
fn apply_does_not_dirty_already_bound_mesh_material_and_publishes_when_shadowed() {
    #[derive(Resource, Default)]
    struct Clock(f32);
    #[derive(Resource, Default)]
    struct ChangedCount(usize);

    let track = MaterialTrack {
        shape: "Glow".into(),
        target: MaterialTarget::EmissiveMult,
        values: Arc::new(ValueTrack::Float(Channel::Keys(Keys {
            kind: KeyKind::Linear,
            keys: vec![
                Key { time: 0.0, value: [1.0], forward: [0.0], backward: [0.0], tbc: [0.0; 3] },
                Key { time: 2.0, value: [0.2], forward: [0.0], backward: [0.0], tbc: [0.0; 3] },
            ],
        }))),
        cycle: None,
        priority: 0,
    };

    let mut app = App::new();
    app.init_resource::<Assets<NifMaterial>>()
        .init_resource::<Clock>()
        .init_resource::<ChangedCount>()
        .add_systems(
            Update,
            (
                bind_rig_materials,
                |mut clock: ResMut<Clock>, mut rigs: Query<&mut RigMaterials>, mut drawing: RigMaterialDrawing| {
                    clock.0 += 0.5;
                    let t = clock.0;
                    for mut rig in &mut rigs {
                        let tracks = rig.idle.clone();
                        let dark = rig.dark;
                        rig.apply(&tracks, t, t, dark, &mut drawing);
                    }
                },
                |q: Query<(), Changed<MeshMaterial3d<NifMaterial>>>, mut count: ResMut<ChangedCount>| {
                    count.0 = q.iter().count();
                },
            )
                .chain(),
        );

    let shared = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(NifMaterial::default());
    let cloak = app.world_mut().resource_mut::<Assets<NifMaterial>>().add(NifMaterial::default());
    let entity = app.world_mut().spawn(MeshMaterial3d(shared.clone())).id();
    app.world_mut().entity_mut(entity).insert(RigMaterials {
        slots: vec![MaterialSlot::new("Glow".into(), entity, shared.clone(), [1.0; 3], 1.0, 1.0)],
        idle: Arc::new(vec![track]),
        dark: false,
        fade: 1.0,
    });

    // First update binds private handle (changed once).
    app.update();
    assert_eq!(app.world().resource::<ChangedCount>().0, 1);

    // Second update animates emissive in place without re-inserting MeshMaterial3d.
    app.update();
    assert_eq!(app.world().resource::<ChangedCount>().0, 0, "unshadowed animated material must not dirty MeshMaterial3d");

    // When a cloak override shadows slot.handle (`current.0 != slot.handle && source == &slot.handle`),
    // apply publishes slot.handle so cloak derivation sees the update.
    let private_handle = app.world().get::<RigMaterials>(entity).unwrap().slots[0].handle.clone();
    app.world_mut().entity_mut(entity).insert((
        MeshMaterial3d(cloak.clone()),
        MaterialOverride { source: private_handle.clone(), copy: cloak.clone() },
    ));
    // Clear change tick from the test's insert by running one unchanged frame? No, next update advances clock
    // and `apply` restores `MeshMaterial3d(private_handle)` via publish.
    app.update();
    assert_eq!(app.world().get::<MeshMaterial3d<NifMaterial>>(entity).unwrap().0, private_handle);
}
