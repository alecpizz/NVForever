//! A compute shader that unpacks bins.
//!
//! This shader runs before mesh preprocessing in order to generate
//! `PreprocessWorkItem`s from cached bins. One invocation of this shader
//! corresponds to one binned entity. Each shader invocation builds the work item
//! for its entity by copying over the mesh input uniform index and calculating
//! the position of the command in the indirect parameters buffer that will draw
//! that entity.
//!
//! NVForever merged scene unpacking: a single dispatch covers every batch set of
//! one (view, phase, mesh indexed-ness). The batch sets are packed into shared
//! buffers (bevy_render gpu_preprocessing.rs `SceneUnpackingClassBuffers`); each
//! workgroup belongs to one batch set (`workgroup_jobs`), whose `first_*` offsets
//! turn its batch-local indices into indices of the packed buffers.

import bevy_render::occlusion_culling::mesh_preprocess_types::{BinMetadata, PreprocessWorkItem};

/// One batch set (`GpuSceneUnpackingJob` on the CPU).
struct SceneUnpackingJob {
    /// The index of the first `PreprocessWorkItem` this batch set writes.
    base_output_work_item_index: u32,
    /// The index of the first GPU indirect parameters command for this batch
    /// set.
    base_indirect_parameters_index: u32,
    /// The number of binned mesh instances of this batch set.
    binned_mesh_instance_count: u32,
    /// Its first entry in `binned_mesh_instances`.
    first_instance: u32,
    /// Its first entry in `bin_metadata`.
    first_bin_metadata: u32,
    /// Its first entry in `bin_index_to_bin_metadata_index`.
    first_bin_index: u32,
    /// Its first workgroup of this dispatch.
    first_unpacking_workgroup: u32,
    /// Fields `allocate_uniforms` reads.
    batch_set_index: u32,
    bin_count: u32,
    first_indirect_parameters_index: u32,
    first_output_mesh_uniform_index: u32,
    first_allocation_workgroup: u32,
};

/// One mesh instance in a bin.
///
/// This corresponds to the CPU-side `GpuRenderBinnedMeshInstance` structure.
///
/// Note that this structure isn't sorted within the
/// `binned_mesh_instances` buffer. Instances from the same bin aren't
/// guaranteed to be adjacent to one another.
struct BinnedMeshInstance {
    /// The index of the `MeshInputUniform` corresponding to this mesh instance
    /// in the `MeshInputUniform` buffer.
    input_uniform_index: u32,
    /// The index of the bin that this mesh instance belongs to.
    bin_index: u32,
};

/// The batch sets.
@group(0) @binding(0) var<storage> jobs: array<SceneUnpackingJob>;

/// The batch set of each workgroup.
@group(0) @binding(1) var<storage> workgroup_jobs: array<u32>;

/// The input array of `BinnedMeshInstance`s.
///
/// Note that this array isn't sorted.
@group(0) @binding(2) var<storage> binned_mesh_instances: array<BinnedMeshInstance>;

/// The output list of `PreprocessWorkItem`s.
@group(0) @binding(3) var<storage, read_write> preprocess_work_items: array<PreprocessWorkItem>;

/// The bin metadata, which contains the index of the GPU indirect parameters for
/// this bin, relative to the start of the indirect parameters for this batch
/// set.
///
/// This is indexed by the bin metadata index below.
@group(0) @binding(4) var<storage> bin_metadata: array<BinMetadata>;

/// A mapping from the bin index to the metadata index within the `bin_metadata`
/// buffer.
@group(0) @binding(5) var<storage> bin_index_to_bin_metadata_index: array<u32>;

@compute
@workgroup_size(64)
fn main(
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
    @builtin(local_invocation_index) local_index: u32,
) {
    // Figure out which batch set and which of its instances we're looking at.
    let job = jobs[workgroup_jobs[workgroup_id.x]];
    let local_id = (workgroup_id.x - job.first_unpacking_workgroup) * 64u + local_index;
    if (local_id >= job.binned_mesh_instance_count) {
        return;
    }

    // Unpack the `BinnedMeshInstance`.
    let instance = binned_mesh_instances[job.first_instance + local_id];

    // Look up the indirect parameters index for this bin, relative to the first
    // indirect parameters offset for this batch set.
    let bin_metadata_index =
        bin_index_to_bin_metadata_index[job.first_bin_index + instance.bin_index];
    let indirect_parameters_offset =
        bin_metadata[job.first_bin_metadata + bin_metadata_index].indirect_parameters_offset;

    // Determine the location we should write the work item to.
    let output_index = job.base_output_work_item_index + local_id;

    // Write out the resulting work item.
    preprocess_work_items[output_index].input_index = instance.input_uniform_index;
    preprocess_work_items[output_index].output_or_indirect_parameters_index =
        job.base_indirect_parameters_index + indirect_parameters_offset;
}
