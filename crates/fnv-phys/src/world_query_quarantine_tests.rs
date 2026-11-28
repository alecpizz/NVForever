// Actual PhysWorld/pipeline/scene-query controls, with original synthetic shapes.
fn box_object(motion: Motion) -> CollisionObject {
    CollisionObject { node: None, node_block: 0, node_transform: Iso::IDENTITY,
        body_transform: Iso::IDENTITY, motion, layer: crate::layers::STATIC,
        mass: 10., friction: 0.5, restitution: 0.,
        shape: Shape::Box { half_extents: [10.; 3] }.into(), shell: 0., material: None, wind: false }
}
fn at(id: u32, x: f32) -> RefPose {
    RefPose { form_id: id, position: [x, 0., 100.],
        rotation: [1.,0.,0.,0.,1.,0.,0.,0.,1.], scale: 1. }
}
fn fixture() -> (PhysWorld, RigidBodyHandle) {
    let mut w = PhysWorld::new(PhysParams { gravity: 0., ..Default::default() });
    w.add_ref(&at(99, 0.), &[box_object(Motion::Dynamic)]);
    w.add_ref(&at(98, 200.), &[box_object(Motion::Dynamic)]);
    w.step();
    let h = w.refs[&99][0];
    assert!(w.w.bodies[h].is_enabled());
    assert!(w.quarantined().is_empty());
    (w, h)
}
fn ray(w: &mut PhysWorld, x: f32) -> Option<RayHit> {
    w.queries().ray([x,0.,200.], [0.,0.,-1.], 300., &Filter::default())
}
fn healthy_ray(w: &mut PhysWorld) {
    assert!(matches!(ray(w,200.).map(|h|h.owner),Some(Owner::Ref {form_id:98,..})),
        "a quarantine cannot hide the healthy scene or return an unfinished BVH");
}
fn time(w: &PhysWorld) -> (u64, f64, f32, f32) {
    (w.steps(), w.stepped_secs(), w.params.dt, w.w.integration_parameters.dt)
}

#[test]
fn query_only_invalid_velocity_is_reported_frozen_and_found_in_the_same_query_without_time() {
    let (mut w,h) = fixture();
    w.set_ref_velocity(98,0,120.);w.step();
    let healthy_drawn = w.drawn_ref_body(98,0,0.5).unwrap();
    let was = w.ref_bodies(99)[0];let clock = time(&w);
    assert_eq!(w.set_ref_velocity(99,0,f32::NAN),1);
    // Velocity alone need not dirty geometry. An actual additional collider makes
    // the next query run registration while the invalid velocity is queued.
    w.add_ref(&at(97,500.),&[box_object(Motion::Static)]);
    assert!(matches!(ray(&mut w,0.).map(|h|h.owner),Some(Owner::Ref {form_id:99,..})),
        "the same query sees the finite frozen remedy, not a disabled phantom");
    assert_eq!(time(&w),clock);
    assert_eq!(w.ref_bodies(99)[0].translation,was.translation);
    assert!(w.w.bodies[h].is_fixed() && w.w.bodies[h].is_enabled());
    assert!(!w.dirty && w.new_bodies.is_empty(),"all remedy flags were republished");
    assert_eq!(w.drawn_ref_body(98,0,0.5),Some(healthy_drawn),"query refresh does not recapture healthy interpolation");
    assert!(!w.drawn_refs().contains(&99));
    assert_eq!(w.quarantined().len(),1);
    assert_eq!(w.quarantined()[0].remedy,crate::quarantine::Remedy::Frozen);
    assert_eq!(w.quarantined()[0].step,clock.0);
    for _ in 0..3 {healthy_ray(&mut w);assert!(ray(&mut w,0.).is_some());}
    assert_eq!((w.stats().quarantined_bodies,w.stats().quarantined_colliders),(1,0));
    w.step();healthy_ray(&mut w);assert!(ray(&mut w,0.).is_some());
    assert_eq!(w.quarantined().len(),1,"next pipeline clear cannot lose or replay the copied report");
}

#[test]
fn query_only_ragdoll_quarantine_rebuilds_at_rest_and_publishes_the_real_actor_ray() {
    use fnv_collide::ragdoll::{Ragdoll,RagdollBody};
    let (mut w,_) = fixture();
    let node = Iso { translation:[-200.,0.,100.], rotation:at(7,0.).rotation };
    let data = std::sync::Arc::new(Ragdoll { bodies:vec![RagdollBody {
        object:CollisionObject {node:Some("Authored bone".into()),node_transform:node,
            motion:Motion::Keyframed,..box_object(Motion::Keyframed)},
        linear_damping:0.1,angular_damping:0.1,stored:node,unit:7.,inertia:None,
    }], joints:vec![],problems:vec![] });
    let built=w.add_ragdoll(&crate::RagdollStart {form_id:7,ragdoll:&data,scale:1.,nodes:&[node],velocity:[0.;3],parts:&[],asleep:false});
    assert_eq!((built.bodies,built.errors.len()),(1,0));w.step();
    let old=w.ragdolls[&7].bodies[0].unwrap();
    w.w.bodies[old].set_linvel(Vector::splat(f32::NAN),true);w.mark_dirty_bodies();
    let clock=time(&w);
    assert!(matches!(ray(&mut w,-200.).map(|h|h.owner),Some(Owner::Actor {form_id:7,..})),
        "the query publishes the rebuilt at-rest body's real collider");
    assert_eq!(time(&w),clock);assert!(w.ragdoll_asleep(7));
    assert!(w.w.bodies.get(old).is_none(),"the actual handler replaced the old ragdoll body");
    assert!(w.ragdoll_nodes(7).unwrap().iter().flatten().all(|b|b.translation.iter().all(|v|v.is_finite())));
    assert!(!w.dirty && w.new_bodies.is_empty());healthy_ray(&mut w);
    assert_eq!(w.quarantined().len(),1);
    assert_eq!(w.quarantined()[0].remedy,crate::quarantine::Remedy::RagdollAtRest);
    for _ in 0..3 {w.update_queries();}w.step();
    assert_eq!(w.quarantined().len(),1);assert!(w.ragdoll_asleep(7));
    assert!(matches!(ray(&mut w,-200.).map(|h|h.owner),Some(Owner::Actor {form_id:7,..})));
}

#[test]
fn query_only_bad_collider_is_reported_once_and_refused_while_healthy_queries_still_work() {
    let (mut w,_) = fixture();
    let owner=Owner::Ref {form_id:0xc100,object:0,layer:crate::layers::STATIC};
    let bad=w.w.insert_collider(ColliderBuilder::ball(f32::NAN).user_data(owner.encode()),None);
    w.mark_dirty_bodies();let clock=time(&w);
    assert!(matches!(ray(&mut w,0.).map(|h|h.owner),Some(Owner::Ref {form_id:99,..})),
        "the same query refuses the invalid collider and still hits the legitimate body in its region");
    healthy_ray(&mut w);
    assert_eq!(time(&w),clock);assert!(!w.w.colliders[bad].is_enabled());
    assert!(!w.dirty);
    assert_eq!(w.quarantined().len(),1);
    assert!(w.quarantined()[0].collider && w.quarantined()[0].owner==Some(owner));
    assert_eq!(w.quarantined()[0].remedy,crate::quarantine::Remedy::Disabled);
    assert_eq!((w.stats().quarantined_bodies,w.stats().quarantined_colliders),(0,1));
    for _ in 0..3 {w.update_queries();healthy_ray(&mut w);}w.step();healthy_ray(&mut w);
    assert_eq!(w.quarantined().len(),1);assert!(!w.w.colliders[bad].is_enabled());
}

#[test]
fn query_only_body_without_a_finite_pose_stays_disabled_and_cannot_become_a_hit() {
    let (mut w,h) = fixture();
    w.w.bodies[h].set_position(Pose::from_translation(Vector::splat(f32::NAN)),true);
    w.mark_dirty_bodies();let clock=time(&w);
    assert!(ray(&mut w,0.).is_none());healthy_ray(&mut w);
    assert_eq!(time(&w),clock);assert!(!w.w.bodies[h].is_enabled());
    assert!(w.quarantined().iter().any(|r|!r.collider && r.owner==Some(Owner::Ref {form_id:99,object:0,layer:crate::layers::STATIC})
        && r.remedy==crate::quarantine::Remedy::Disabled));
    let reports=w.quarantined().len();let counts=(w.stats().quarantined_bodies,w.stats().quarantined_colliders);
    w.step();assert!(ray(&mut w,0.).is_none());healthy_ray(&mut w);
    assert_eq!(w.quarantined().len(),reports);assert_eq!((w.stats().quarantined_bodies,w.stats().quarantined_colliders),counts);
}

#[test]
fn positive_step_quarantine_keeps_new_remedy_marks_until_the_next_query_registers_them() {
    let (mut w,h) = fixture();let before=time(&w);
    assert_eq!(w.set_ref_velocity(99,0,f32::NAN),1);w.step();
    assert_eq!(w.steps(),before.0+1);assert_eq!(w.stepped_secs(),before.1+f64::from(before.2));
    assert!(w.dirty,"the positive step must not erase the handler's new registration mark");
    assert!(w.w.bodies[h].is_fixed() && w.w.bodies[h].is_enabled());
    assert_eq!(w.quarantined().len(),1);let clock=time(&w);
    assert!(matches!(ray(&mut w,0.).map(|h|h.owner),Some(Owner::Ref {form_id:99,..})));
    assert_eq!(time(&w),clock);assert!(!w.dirty);healthy_ray(&mut w);
    w.step();assert_eq!(w.quarantined().len(),1);
}

#[test]
fn ordinary_refresh_preserves_interpolation_pending_kinematic_target_and_user_dynamics() {
    let (mut w,h) = fixture();
    // add_ref intentionally turns Motion::Keyframed into a fixed, teleported
    // wrapper body. Use a real position-based native body, as the unchanged
    // queued-motion contract does; do not relabel a fixed body as kinematic.
    let owner=Owner::Ref {form_id:200,object:0,layer:crate::layers::STATIC}.encode();
    let k=w.insert_body(RigidBodyBuilder::kinematic_position_based()
        .translation(Vector::new(400.,0.,100.)*w.k).user_data(owner));
    let collider=w.w.insert_collider(ColliderBuilder::cuboid(10.*w.k,10.*w.k,10.*w.k)
        .active_collision_types(ActiveCollisionTypes::all()).user_data(owner),Some(k));
    assert_eq!(w.w.bodies[k].body_type(),RigidBodyType::KinematicPositionBased);
    assert_eq!(w.w.colliders[collider].parent(),Some(k));
    w.mark_dirty_bodies();w.step();
    let current=*w.w.bodies[k].position();let target=current.translation+Vector::X;
    w.w.bodies[k].set_next_kinematic_translation(target);
    w.w.bodies[h].add_force(Vector::new(3.,4.,5.),true);
    let force=w.w.bodies[h].user_force();let body=w.ref_bodies(99)[0];let drawn=w.drawn_ref_body(99,0,0.5);let clock=time(&w);
    w.mark_dirty_bodies();w.update_queries();
    assert_eq!(time(&w),clock);assert_eq!(w.ref_bodies(99)[0],body);assert_eq!(w.drawn_ref_body(99,0,0.5),drawn);
    assert_eq!(*w.w.bodies[k].position(),current);assert_eq!(w.w.bodies[k].next_position().translation,target);
    assert_eq!(w.w.bodies[h].user_force(),force);assert!(w.quarantined().is_empty());assert!(!w.dirty);
    healthy_ray(&mut w);w.step();
    assert_eq!(w.w.bodies[k].position().translation,target);assert_ne!(w.ref_bodies(99)[0].linvel,body.linvel);
    assert_eq!(w.steps(),clock.0+1);assert!(w.quarantined().is_empty());
}

// Post-fix neighbor coverage: no pre-fix execution is claimed for this case.
#[test]
fn query_only_invalid_kinematic_target_is_reenabled_found_and_can_move_on_the_next_step() {
    let (mut w,_) = fixture();
    let owner=Owner::Ref {form_id:200,object:0,layer:crate::layers::STATIC};
    let k=w.insert_body(RigidBodyBuilder::kinematic_position_based()
        .translation(Vector::new(400.,0.,100.)*w.k).user_data(owner.encode()));
    let collider=w.w.insert_collider(ColliderBuilder::cuboid(10.*w.k,10.*w.k,10.*w.k)
        .active_collision_types(ActiveCollisionTypes::all()).user_data(owner.encode()),Some(k));
    w.mark_dirty_bodies();w.step();
    assert_eq!(w.w.bodies[k].body_type(),RigidBodyType::KinematicPositionBased);
    assert_eq!(w.w.colliders[collider].parent(),Some(k));
    assert_eq!(ray(&mut w,400.).map(|h|h.owner),Some(owner),
        "the real parented collider is visible through the default owner filter before poisoning");
    assert!(w.quarantined().is_empty());
    let current=*w.w.bodies[k].position();let clock=time(&w);
    w.w.bodies[k].set_next_kinematic_translation(Vector::splat(f32::NAN));
    assert!(w.w.bodies[k].position().is_finite());
    assert!(!w.w.bodies[k].next_position().is_finite(),"the actual native target is invalid");
    w.mark_dirty_bodies();
    assert_eq!(ray(&mut w,400.).map(|h|h.owner),Some(owner),
        "the same query republishes the Enabled remedy's actual collider");
    assert_eq!(time(&w),clock);
    assert_eq!(*w.w.bodies[k].position(),current);
    assert_eq!(*w.w.bodies[k].next_position(),current);
    assert_eq!(w.w.bodies[k].body_type(),RigidBodyType::KinematicPositionBased);
    assert!(w.w.bodies[k].is_enabled() && w.w.colliders[collider].is_enabled());
    assert_eq!(w.w.bodies[k].linvel(),Vector::ZERO);
    assert!(!w.dirty && w.new_bodies.is_empty(),"re-enable flags are registered before returning the hit");
    assert_eq!(w.quarantined().len(),1);
    let report=w.quarantined()[0];
    assert_eq!(report.owner,Some(owner));assert!(!report.collider);
    assert_eq!(report.remedy,crate::quarantine::Remedy::Enabled);
    assert_eq!(report.step,clock.0);
    let at=report.at.expect("finite current pose is reported in game units");
    for (actual,expected) in at.into_iter().zip([400.,0.,100.]) {assert!((actual-expected).abs()<0.001);}
    assert_eq!((w.stats().quarantined_bodies,w.stats().quarantined_colliders),(1,0));
    for _ in 0..3 {
        assert_eq!(ray(&mut w,400.).map(|h|h.owner),Some(owner));healthy_ray(&mut w);
        assert_eq!(w.quarantined(),&[report]);assert_eq!(time(&w),clock);
    }
    w.step();
    assert_eq!(*w.w.bodies[k].position(),current);
    assert_eq!(w.steps(),clock.0+1);
    assert_eq!(w.stepped_secs(),clock.1+f64::from(clock.2));
    assert_eq!(ray(&mut w,400.).map(|h|h.owner),Some(owner));
    assert_eq!(w.quarantined(),&[report]);
    // Re-enabling preserves the kinematic owner: its next valid target really moves.
    let target=current.translation+Vector::X;
    w.w.bodies[k].set_next_kinematic_translation(target);w.step();
    assert_eq!(w.w.bodies[k].position().translation,target);
    assert!(w.w.bodies[k].position().is_finite() && w.w.bodies[k].is_enabled());
    let target_x=target.x/w.k;
    assert_eq!(ray(&mut w,target_x).map(|h|h.owner),Some(owner));healthy_ray(&mut w);
    assert_eq!(w.quarantined(),&[report]);
    assert_eq!((w.stats().quarantined_bodies,w.stats().quarantined_colliders),(1,0));
}
