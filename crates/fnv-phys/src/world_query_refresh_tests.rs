mod quarantine_controls {
    use super::*;
    include!("world_query_quarantine_tests.rs");
}

use crate::*;
use fnv_collide::{CollisionObject, Iso, Motion, Shape};
use rapier3d::prelude::*;

fn shape(motion:Motion) -> CollisionObject {
    CollisionObject {node:None,node_block:0,node_transform:Iso::IDENTITY,body_transform:Iso::IDENTITY,
        motion,layer:1,mass:10.,friction:0.5,restitution:0.,shape:Shape::Box {half_extents:[10.;3]}.into(),shell:0.,material:None,wind:false}
}
fn pose(id:u32, position:[f32;3])->RefPose {RefPose {form_id:id,position,rotation:[1.,0.,0.,0.,1.,0.,0.,0.,1.],scale:1.}}
fn world()->(PhysWorld,RigidBodyHandle) {
    let mut w=PhysWorld::new(PhysParams {gravity:0.,..Default::default()});
    w.add_ref(&pose(99,[1000.,0.,500.]),&[shape(Motion::Dynamic)]);
    w.add_ref(&pose(98,[0.;3]),&[CollisionObject {shape:Shape::Box {half_extents:[70.;3]}.into(),..shape(Motion::Static)}]);
    w.step();let handle=w.refs[&99][0];assert!(!w.w.bodies[handle].is_sleeping());assert_eq!(w.stats().bodies_awake,1);(w,handle)
}

#[test]
fn nonzero_user_torque_survives_query_and_produces_later_angular_motion() {
    let (mut w,h)=world();
    w.w.bodies[h].add_torque(rapier3d::math::Vector::new(1.,2.,3.),true);
    let torque=w.w.bodies[h].user_torque();let angular=w.w.bodies[h].angvel();let body=w.ref_bodies(99)[0];let steps=w.steps();
    assert!(torque.length_squared()>0.);
    for _ in 0..3 {w.mark_dirty_bodies();w.update_queries();assert_eq!(w.steps(),steps);assert_eq!(w.ref_bodies(99)[0],body);assert_eq!(w.w.bodies[h].user_torque(),torque);assert_eq!(w.w.bodies[h].angvel(),angular);}
    w.step();assert!((w.w.bodies[h].angvel()-angular).length_squared()>0.,"nonzero queued torque acts only on the later real step");
}

#[test]
fn actually_sleeping_body_struck_while_new_asleep_clutter_is_pending_keeps_both_intents() {
    let (mut w,h)=world();for _ in 0..240 {w.step();}
    assert!(w.w.bodies[h].is_sleeping(),"real simulation put the initially awake body to sleep");
    w.insert_ref(PreparedRef::new(&w.params,&pose(100,[3000.,0.,500.]),&[shape(Motion::Dynamic)]).asleep());
    let fresh=w.ref_bodies(100)[0];assert!(fresh.sleeping);assert!(!w.pending_sleep.is_empty());
    let sleeping=w.ref_bodies(99)[0];
    assert!(w.push_ref(99,sleeping.translation,[400.,0.,0.],1000.).is_some());
    assert!(w.struck.contains(&h));let pushed=w.ref_bodies(99)[0];assert!(!pushed.sleeping);assert!(pushed.linvel[0]>0.);
    let steps=w.steps();w.update_queries();assert_eq!(w.steps(),steps);assert_eq!(w.ref_bodies(99)[0],pushed);
    assert_eq!(w.ref_bodies(100)[0],fresh);assert!(w.pending_sleep.is_empty());
    w.step();assert!(w.ref_bodies(99)[0].translation[0]>pushed.translation[0]);assert_eq!(w.ref_bodies(100)[0],fresh);
}

#[test]
fn new_real_position_and_velocity_kinematic_contacts_keep_unintegrated_motion_targets() {
    for position_based in [true,false] {
        let (mut w,_)=world();
        let builder=if position_based {RigidBodyBuilder::kinematic_position_based()} else {RigidBodyBuilder::kinematic_velocity_based()};
        let h=w.insert_body(builder.translation(rapier3d::math::Vector::new(0.,0.,1.15)));
        let collider=w.w.insert_collider(ColliderBuilder::cuboid(0.25,0.25,0.25).active_collision_types(ActiveCollisionTypes::all()),Some(h));
        if position_based {w.w.bodies[h].set_next_kinematic_translation(rapier3d::math::Vector::new(1.,0.,1.15));}
        else {w.w.bodies[h].set_linvel(rapier3d::math::Vector::new(0.5,0.,0.),false);}
        let current=*w.w.bodies[h].position();let target=*w.w.bodies[h].next_position();let velocity=w.w.bodies[h].linvel();let steps=w.steps();
        w.mark_dirty_bodies();w.update_queries();
        assert_eq!(w.steps(),steps);assert_eq!(*w.w.bodies[h].position(),current);assert_eq!(w.w.bodies[h].linvel(),velocity);
        assert!(w.w.contact_pairs_with(collider).next().is_some(),"real narrow-phase contact with the fixed support");
        assert_eq!(w.stats().bodies_awake,2,"old dynamic plus new real kinematic registered");
        assert_eq!(*w.w.bodies[h].next_position(),target,"query cannot consume a queued position target");
        w.step();assert!(w.w.bodies[h].position().translation.x>current.translation.x);
    }
}
