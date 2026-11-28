use fnv_collide::{CollisionObject, Iso, Motion, Shape};
use fnv_phys::{PhysParams, PhysWorld, RefPose};
fn add(world: &mut PhysWorld, id: u32, position: [f32; 3], half: f32, motion: Motion) {
    let pose=RefPose { form_id:id, position, rotation:[1.,0.,0.,0.,1.,0.,0.,0.,1.], scale:1. };
    let shape=CollisionObject { node:None,node_block:0,node_transform:Iso::IDENTITY,body_transform:Iso::IDENTITY,
        motion,layer:1,mass:10.,friction:0.5,restitution:0.,shape:Shape::Box {half_extents:[half;3]}.into(),
        shell:0.,material:None,wind:false };
    assert_eq!(world.add_ref(&pose,&[shape]).bodies,1);
}
fn scene() -> PhysWorld {
    let mut w=PhysWorld::new(PhysParams { gravity:0.,..Default::default() });
    add(&mut w,1,[1000.,1000.,500.],10.,Motion::Dynamic);
    add(&mut w,2,[0.,0.,0.],50.,Motion::Static);
    w.step();
    assert!(!w.ref_bodies(1)[0].sleeping);
    assert_eq!(w.stats().bodies_awake,1);
    w
}
#[test]
fn refreshed_new_dynamic_contact_has_registered_active_population_without_time_advance() {
    let mut w=scene();
    add(&mut w,3,[0.,0.,55.],10.,Motion::Dynamic);
    let steps=w.steps();let before=w.ref_bodies(3)[0];
    assert!(!before.sleeping);
    w.update_queries();
    assert_eq!(w.steps(),steps,"query refresh never integrates time");
    assert_eq!(w.ref_bodies(3)[0],before,"query refresh preserves the authored pose/velocity");
    assert!(!w.ref_contacts(3).is_empty(),"actual new touching contact reached narrow phase");
    let awake=(1..=3).flat_map(|id|w.ref_bodies(id)).filter(|body|!body.sleeping).count()-1; // exclude sole fixed body
    assert_eq!(awake,2,"both actual dynamic bodies are awake");
    eprintln!("actual awake dynamic bodies={awake}; registered active population={}; steps={steps}",w.stats().bodies_awake);
    assert_eq!(w.stats().bodies_awake,awake,"enabled awake nonfixed contact endpoint must be island-registered before detection");
}
#[test]
fn repeated_refresh_of_existing_registered_bodies_keeps_time_and_population() {
    let mut w=scene();let body=w.ref_bodies(1)[0];let steps=w.steps();
    for _ in 0..3 {w.update_queries();assert_eq!(w.stats().bodies_awake,1);assert_eq!(w.ref_bodies(1)[0],body);assert_eq!(w.steps(),steps);}
}

#[test]
fn newly_inserted_asleep_clutter_stays_asleep_across_query_and_normal_step() {
    let mut w=scene();
    let pose=RefPose { form_id:4,position:[0.,0.,55.],rotation:[1.,0.,0.,0.,1.,0.,0.,0.,1.],scale:1. };
    let shape=CollisionObject {node:None,node_block:0,node_transform:Iso::IDENTITY,body_transform:Iso::IDENTITY,
        motion:Motion::Dynamic,layer:1,mass:10.,friction:0.5,restitution:0.,shape:Shape::Box {half_extents:[10.;3]}.into(),shell:0.,material:None,wind:false};
    w.insert_ref(fnv_phys::PreparedRef::new(&PhysParams::default(),&pose,&[shape]).asleep());
    let before=w.ref_bodies(4)[0];assert!(before.sleeping);
    let steps=w.steps();w.update_queries();
    assert_eq!(w.steps(),steps);assert_eq!(w.ref_bodies(4)[0],before);
    for _ in 0..20 {w.step();}
    assert_eq!(w.ref_bodies(4)[0],before,"authored asleep overlapping shelf remains preserved");
}

#[test]
fn query_refresh_preserves_impulse_velocity_and_later_normal_motion() {
    let mut w=scene();
    let before=w.ref_bodies(1)[0];
    assert!(w.push_ref(1,before.translation,[400.,0.,0.],1000.).is_some());
    let pushed=w.ref_bodies(1)[0];assert!(pushed.linvel[0]>before.linvel[0]);
    let steps=w.steps();w.update_queries();
    assert_eq!(w.steps(),steps);assert_eq!(w.ref_bodies(1)[0],pushed);
    for _ in 0..10 {w.step();}
    assert!(w.ref_bodies(1)[0].translation[0]>pushed.translation[0]);
}

#[test]
fn newly_inserted_jointed_object_refresh_preserves_bind_pose_and_later_joint_constraint() {
    use fnv_collide::ragdoll::{Ragdoll,RagdollBody,RagdollJoint,JointKind};
    use std::sync::Arc;
    let mut w=scene();
    let body=|name:&str,x:f32,motion| {
        let rest=Iso {translation:[x,0.,55.],rotation:[1.,0.,0.,0.,1.,0.,0.,0.,1.]};
        RagdollBody {object:CollisionObject {node:Some(name.into()),node_block:0,node_transform:rest,body_transform:Iso::IDENTITY,
            motion,layer:1,mass:10.,friction:0.5,restitution:0.,shape:Shape::Box {half_extents:[5.;3]}.into(),shell:0.,material:None,wind:false},
            linear_damping:0.1,angular_damping:0.05,stored:rest,unit:7.,inertia:None}
    };
    let rag=Arc::new(Ragdoll {bodies:vec![body("owned anchor",0.,Motion::Static),body("owned arm",20.,Motion::Dynamic)],
        joints:vec![RagdollJoint {block:0,a:0,b:1,pivot_a:[20.,0.,0.],pivot_b:[0.;3],kind:JointKind::Ball,max_friction:0.,strength:None}],problems:vec![]});
    let nodes:Vec<_>=rag.bodies.iter().map(|b|b.rest()).collect();
    let report=w.add_object_ragdoll(&fnv_phys::RagdollStart {form_id:5,ragdoll:&rag,scale:1.,nodes:&nodes,velocity:[30.,0.,0.],parts:&[],asleep:false});
    assert_eq!((report.bodies,report.joints),(2,1));assert!(report.errors.is_empty());
    let before=w.ragdoll_nodes(5).unwrap();let steps=w.steps();
    w.update_queries();assert_eq!(w.steps(),steps);assert_eq!(w.ragdoll_nodes(5).unwrap(),before);
    assert_eq!(w.stats().joints,1);
    for _ in 0..30 {w.step();}
    let arm=w.ragdoll_nodes(5).unwrap()[1].unwrap();
    assert!((arm.translation[0]-20.).abs()<1.,"real ball joint holds its center against the initial outward velocity: {arm:?}");
}
