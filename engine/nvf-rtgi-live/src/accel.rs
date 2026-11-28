//! The acceleration structures: one bottom level structure of all triangles of the scene, those with an alpha test as a
//! geometry of their own, and a top level one of it alone. Built once, for tracing; never compacted (wgpu-core 30.0.1
//! locks in opposite orders there and in submit) nor refitted.

use crate::tables::Tables;
use std::future::Future;

const FLAGS: wgpu::AccelerationStructureFlags = wgpu::AccelerationStructureFlags::PREFER_FAST_TRACE;
const SAME: [f32; 12] = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0];

/// `make`, or the errors it left on the device.
pub fn scoped<T>(device: &wgpu::Device, what: &str, make: impl FnOnce() -> T) -> Result<T, String> {
    let filters = [wgpu::ErrorFilter::Validation, wgpu::ErrorFilter::OutOfMemory, wgpu::ErrorFilter::Internal];
    let mut scopes: Vec<_> = filters.map(|f| device.push_error_scope(f)).into();
    let out = make();
    let mut found = Vec::new();
    while let Some(scope) = scopes.pop() {
        // Ready at once where wgpu is native.
        let mut future = std::pin::pin!(scope.pop());
        match future.as_mut().poll(&mut std::task::Context::from_waker(std::task::Waker::noop())) {
            std::task::Poll::Ready(Some(e)) => found.push(format!("{what}: {e}")),
            std::task::Poll::Ready(None) => {}
            std::task::Poll::Pending => found.push(format!("{what}: its errors are not known")),
        }
    }
    if found.is_empty() {
        Ok(out)
    } else {
        Err(found.join("; "))
    }
}

/// Why a device (as it was made) has no kernel.
pub fn supported(device: &wgpu::Device) -> Result<(), String> {
    if !device.features().contains(wgpu::Features::EXPERIMENTAL_RAY_QUERY) {
        return Err("the device has no ray queries".into());
    }
    let l = device.limits();
    // Six tables and what is written; the uniform, the structure.
    within(&[
        ("acceleration structures in a stage", 1, u64::from(l.max_acceleration_structures_per_shader_stage)),
        ("buffers and acceleration structures in a stage", 9, u64::from(l.max_buffers_and_acceleration_structures_per_shader_stage)),
        ("storage buffers in a stage", 7, u64::from(l.max_storage_buffers_per_shader_stage)),
        ("uniform buffers in a stage", 1, u64::from(l.max_uniform_buffers_per_shader_stage)),
        ("uniforms with an offset", 1, u64::from(l.max_dynamic_uniform_buffers_per_pipeline_layout)),
        ("as a binding's number", 8, u64::from(l.max_bindings_per_bind_group) - 1),
        ("bytes of a workgroup", 64 * 64, u64::from(l.max_compute_workgroup_storage_size)),
        ("invocations of a workgroup", 64, u64::from(l.max_compute_invocations_per_workgroup.min(l.max_compute_workgroup_size_x))),
        ("bytes between a uniform's offsets", u64::from(l.min_uniform_buffer_offset_alignment), 256),
    ])
}

fn within(needs: &[(&str, u64, u64)]) -> Result<(), String> {
    match needs.iter().find(|(_, needed, most)| needed > most) {
        Some((what, needed, most)) => Err(format!("oversized: {needed} {what}, the device allows {most}")),
        None => Ok(()),
    }
}

/// Why a device cannot hold a scene.
pub fn fits(device: &wgpu::Device, tables: &Tables) -> Result<(), String> {
    let geometries = u64::from(tables.opaque() > 0) + u64::from(tables.opaque() < tables.triangles());
    let l = device.limits();
    let most = l.max_buffer_size.min(l.max_storage_buffer_binding_size);
    let bytes = |n: usize| 4 * n as u64;
    within(&[
        // wgpu-core holds the vertices to the primitives' limit too.
        ("corners in a structure", tables.corners().len() as u64, u64::from(l.max_blas_primitive_count)),
        ("geometries in a structure", geometries, u64::from(l.max_blas_geometry_count)),
        ("instances", 1, u64::from(l.max_tlas_instance_count)),
        ("bytes of corners", bytes(3 * tables.corners().len()), most),
        ("bytes of faces", bytes(tables.faces().len()), most),
        ("bytes of data", bytes(tables.data().len()), most),
        ("bytes of texels", bytes(tables.texels().len()), most),
    ])
}

pub(crate) struct Accel {
    pub tlas: wgpu::Tlas,
    blas: wgpu::Blas,
    /// With the first corner of each.
    geometries: Vec<(u32, wgpu::BlasTriangleGeometrySizeDescriptor)>,
}

impl Accel {
    /// Not built yet ([`Accel::record`]).
    pub fn new(device: &wgpu::Device, tables: &Tables) -> Result<Accel, String> {
        fits(device, tables)?;
        let opaque = 3 * tables.opaque();
        let all = tables.corners().len() as u32;
        let flags = |opaque| if opaque { wgpu::AccelerationStructureGeometryFlags::OPAQUE } else { wgpu::AccelerationStructureGeometryFlags::empty() };
        let size = |corners, opaque| wgpu::BlasTriangleGeometrySizeDescriptor { vertex_format: wgpu::VertexFormat::Float32x3, vertex_count: corners, index_format: None, index_count: None, flags: flags(opaque) };
        let geometries: Vec<_> = [(0, size(opaque, true)), (opaque, size(all - opaque, false))].into_iter().filter(|g| g.1.vertex_count > 0).collect();
        let update_mode = wgpu::AccelerationStructureUpdateMode::Build;
        let (tlas, blas) = scoped(device, "making the structures", || {
            let blas = device.create_blas(
                &wgpu::CreateBlasDescriptor { label: Some("rtgi_live_blas"), flags: FLAGS, update_mode },
                wgpu::BlasGeometrySizeDescriptors::Triangles { descriptors: geometries.iter().map(|g| g.1.clone()).collect() },
            );
            let mut tlas = device.create_tlas(&wgpu::CreateTlasDescriptor { label: Some("rtgi_live_tlas"), max_instances: 1, flags: FLAGS, update_mode });
            tlas[0] = Some(wgpu::TlasInstance::new(&blas, SAME, 0, 0xff));
            (tlas, blas)
        })?;
        Ok(Accel { tlas, blas, geometries })
    }

    /// Records both builds (the bottom level first, which wgpu does within one call).
    pub fn record(&self, encoder: &mut wgpu::CommandEncoder, corners: &wgpu::Buffer) {
        let geometries = self.geometries.iter().map(|(first, size)| wgpu::BlasTriangleGeometry {
            size,
            vertex_buffer: corners,
            first_vertex: *first,
            vertex_stride: 12,
            index_buffer: None,
            first_index: None,
            transform_buffer: None,
            transform_buffer_offset: None,
        });
        let entry = wgpu::BlasBuildEntry { blas: &self.blas, geometry: wgpu::BlasGeometries::TriangleGeometries(geometries.collect()) };
        encoder.build_acceleration_structures([&entry], [&self.tlas]);
    }
}
