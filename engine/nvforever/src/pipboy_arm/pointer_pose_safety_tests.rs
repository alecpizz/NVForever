// Direct controls for pointer_pose's explicit Mat4 boundary. GlobalTransform is
// affine by type; a projective world cannot be injected through the App fixture.
mod pointer_pose_safety_regressions {
    use super::*;
    fn skinned_square(weights:[f32;4])->Mesh {
        let mut mesh=(*square()).clone();
        mesh.insert_attribute(Mesh::ATTRIBUTE_JOINT_INDEX,bevy::mesh::VertexAttributeValues::Uint16x4(vec![[0_u16;4];4]));
        mesh.insert_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT,vec![weights;4]);mesh
    }
    #[test]
    fn actual_preflight_rejects_zero_nonunit_and_projective_weighted_rows_before_pose_transform() {
        for weights in [[0.0;4],[0.5,0.0,0.0,0.0],[1.5,0.0,0.0,0.0]] {
            assert!(pointer_pose(&skinned_square(weights),Some(&[Mat4::IDENTITY]),Mat4::IDENTITY).is_none(),"finite invalid homogeneous weight row must fail before glam affine transform");
        }
        let mesh=skinned_square([1.0,0.0,0.0,0.0]);
        let mut projective=Mat4::IDENTITY;projective.x_axis.w=0.25;
        assert!(pointer_pose(&mesh,Some(&[projective]),Mat4::IDENTITY).is_none(),"finite non-affine joint cannot enter Mat4::transform_point3");
        let mut nonfinite=Mat4::IDENTITY;nonfinite.z_axis.w=f32::NAN;
        assert!(pointer_pose(&mesh,Some(&[nonfinite]),Mat4::IDENTITY).is_none());
        let valid=pointer_pose(&mesh,Some(&[Mat4::IDENTITY]),Mat4::IDENTITY).expect("normalized affine skin remains eligible");
        let rigid=pointer_pose(&mesh,None,Mat4::IDENTITY).unwrap();assert_eq!(valid.points,rigid.points);assert_eq!(valid.triangles,rigid.triangles);
    }
    #[test]
    fn actual_world_matrix_preflight_rejects_projective_and_nonfinite_but_keeps_affine_rigid_control() {
        let mesh=square();let mut projective=Mat4::IDENTITY;projective.y_axis.w=0.25;
        assert!(pointer_pose(&mesh,None,projective).is_none(),"explicit projective world is outside the helper contract");
        let mut nonfinite=Mat4::IDENTITY;nonfinite.w_axis.x=f32::INFINITY;
        assert!(pointer_pose(&mesh,None,nonfinite).is_none());
        let world=Mat4::from_translation(Vec3::new(3.0,4.0,-10.0));
        let valid=pointer_pose(&mesh,None,world).expect("ordinary affine world remains eligible");
        let identity=pointer_pose(&mesh,None,Mat4::IDENTITY).unwrap();
        for (point,source) in valid.points.iter().zip(identity.points) {assert_eq!(*point,source+Vec3::new(3.0,4.0,-10.0));}
    }
}
