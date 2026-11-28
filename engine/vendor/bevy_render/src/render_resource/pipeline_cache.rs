use bevy_material::descriptor::{
    BindGroupLayoutDescriptor, CachedComputePipelineId, ComputePipelineDescriptor,
    MeshPipelineDescriptor, PipelineDescriptor, RenderPipelineDescriptor,
};
use smallvec::SmallVec;

use crate::{
    render_resource::*,
    renderer::{wgpu_wrapper, RenderDevice},
    Extract,
};
use alloc::{borrow::Cow, sync::Arc};
use bevy_asset::{AssetEvent, AssetId, Assets, Handle};
use bevy_ecs::{
    message::MessageReader,
    resource::Resource,
    system::{Res, ResMut},
};
use bevy_log::error;
use bevy_platform::collections::{hash_map::RawEntryMut, HashMap, HashSet};
use bevy_shader::{
    CachedPipelineId, ModuleStep, Shader, ShaderCache, ShaderCacheError, ShaderCacheSource,
    ShaderDefVal, ValidateShader,
};
use super::nvf_manifest::{self, Asked, Claim, PipelineManifest, Recipe};
use bevy_tasks::Task;
use bevy_utils::default;
use core::{future::Future, mem};
use std::sync::{Mutex, PoisonError};
use wgpu::{PipelineCompilationOptions, VertexBufferLayout as RawVertexBufferLayout};

/// NVForever pipeline timing (test aid `NVF_PLAY_TEST_PIPELINE_LOG`, nvforever
/// `pipeline_gate.rs`): where one pipeline's compile time goes. `queued` is the wait for a
/// task thread, `lock` the wait for the shader cache's lock, `compose` the WESL composition
/// of its shaders not composed before (with `defs`), `naga` the part of that spent in
/// `create_shader_module` (naga's WGSL parse and validation, its SPIR-V output and
/// `vkCreateShaderModule`), and `create` the pipeline's creation (the driver's compile).
pub(crate) mod nvf_timing {
    use core::cell::Cell;
    use std::sync::OnceLock;
    use std::time::{Duration, Instant};

    std::thread_local! {
        /// Time in `load_module` on this thread, and the modules made.
        pub(crate) static MODULES: Cell<(Duration, u32)> = const { Cell::new((Duration::ZERO, 0)) };
    }

    pub(crate) fn enabled() -> bool {
        static ON: OnceLock<bool> = OnceLock::new();
        *ON.get_or_init(|| std::env::var_os("NVF_PLAY_TEST_PIPELINE_LOG").is_some())
    }

    pub(crate) fn modules() -> (Duration, u32) {
        MODULES.with(Cell::get)
    }

    /// One pipeline's spans, logged when it is created.
    pub(crate) struct Spans {
        pub spawned: Instant,
        pub started: Option<Instant>,
        pub locked: Option<Instant>,
        pub shaders: Option<Instant>,
        pub modules_before: (Duration, u32),
        pub modules_after: (Duration, u32),
    }

    impl Spans {
        pub(crate) fn new() -> Self {
            Self { spawned: Instant::now(), started: None, locked: None, shaders: None, modules_before: (Duration::ZERO, 0), modules_after: (Duration::ZERO, 0) }
        }
        pub(crate) fn start(&mut self) {
            self.started = Some(Instant::now());
        }
        pub(crate) fn locked(&mut self) {
            self.locked = Some(Instant::now());
            self.modules_before = modules();
        }
        pub(crate) fn shaders_done(&mut self) {
            self.shaders = Some(Instant::now());
            self.modules_after = modules();
        }
        pub(crate) fn log(&self, label: Option<&str>) {
            let ms = |a: Option<Instant>, b: Option<Instant>| match (a, b) {
                (Some(a), Some(b)) => b.saturating_duration_since(a).as_secs_f64() * 1000.0,
                _ => 0.0,
            };
            let done = Some(Instant::now());
            let naga = self.modules_after.0.saturating_sub(self.modules_before.0).as_secs_f64() * 1000.0;
            let made = self.modules_after.1.saturating_sub(self.modules_before.1);
            let shaders = ms(self.locked, self.shaders);
            super::nvf_add_pipeline_time([
                ms(Some(self.spawned), self.started),
                ms(self.started, self.locked),
                (shaders - naga).max(0.0),
                naga,
                ms(self.shaders, done),
            ]);
            if !enabled() {
                return;
            }
            bevy_log::info!(
                "pipeline timing: queued {:.1} ms, lock {:.1} ms, compose {:.1} ms, naga {:.1} ms ({} modules), create {:.1} ms: {}",
                ms(Some(self.spawned), self.started),
                ms(self.started, self.locked),
                (shaders - naga).max(0.0),
                naga,
                made,
                ms(self.shaders, done),
                label.unwrap_or("(no label)")
            );
        }
    }
}

/// NVForever pipeline timing: the time of every pipeline the game asked for and that was
/// compiled for it (not those the pipeline list's pool built ahead), summed over the run
/// and the threads, as `nvf_timing::Spans` splits it (#2407: player reports' loads wait for
/// "pipelines" 15 s and more on machines whose driver cache is warm, and nothing said
/// whether the time went waiting for a compile thread, for the shader cache's lock, in
/// composing, in naga or in the driver). Read it twice and take [`NvfPipelineTime::since`]
/// for a stretch of the run.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NvfPipelineTime {
    /// Pipelines finished.
    pub pipelines: u64,
    /// Milliseconds: waiting for a compile thread, for the shader cache's lock, composing,
    /// making shader modules (naga and `vkCreateShaderModule`), and creating the pipeline
    /// (the driver's compile, or the wait for the list's pool to hand it over).
    pub queued_ms: f64,
    pub lock_ms: f64,
    pub compose_ms: f64,
    pub naga_ms: f64,
    pub create_ms: f64,
}

impl NvfPipelineTime {
    /// What was added after `before`.
    pub fn since(&self, before: &NvfPipelineTime) -> NvfPipelineTime {
        NvfPipelineTime {
            pipelines: self.pipelines.saturating_sub(before.pipelines),
            queued_ms: (self.queued_ms - before.queued_ms).max(0.0),
            lock_ms: (self.lock_ms - before.lock_ms).max(0.0),
            compose_ms: (self.compose_ms - before.compose_ms).max(0.0),
            naga_ms: (self.naga_ms - before.naga_ms).max(0.0),
            create_ms: (self.create_ms - before.create_ms).max(0.0),
        }
    }
}

/// The sums of [`NvfPipelineTime`]: pipelines, then each part in microseconds.
static NVF_PIPELINE_TIME: [core::sync::atomic::AtomicU64; 6] = [const { core::sync::atomic::AtomicU64::new(0) }; 6];

fn nvf_add_pipeline_time(ms: [f64; 5]) {
    use core::sync::atomic::Ordering::Relaxed;
    NVF_PIPELINE_TIME[0].fetch_add(1, Relaxed);
    for (slot, v) in NVF_PIPELINE_TIME[1..].iter().zip(ms) {
        slot.fetch_add((v * 1000.0) as u64, Relaxed);
    }
}

/// The pipeline time so far in this run ([`NvfPipelineTime`]).
pub fn nvf_pipeline_time() -> NvfPipelineTime {
    use core::sync::atomic::Ordering::Relaxed;
    let us = |i: usize| NVF_PIPELINE_TIME[i].load(Relaxed) as f64 / 1000.0;
    NvfPipelineTime { pipelines: NVF_PIPELINE_TIME[0].load(Relaxed), queued_ms: us(1), lock_ms: us(2), compose_ms: us(3), naga_ms: us(4), create_ms: us(5) }
}

/// A pipeline defining the data layout and shader logic for a specific GPU task.
///
/// Used to store a heterogenous collection of render and compute pipelines together.
#[derive(Debug)]
pub enum Pipeline {
    RenderPipeline(RenderPipeline),
    ComputePipeline(ComputePipeline),
    MeshPipeline(RenderPipeline),
}

/// NVForever Streamline integration: pipeline compilation and external device polling hold
/// the read side. DLSS-G's live load/unload holds the write side, excluding driver calls
/// from task threads, including tasks cancelled by a shader reload.
/// [documented: Streamline `sl_core_api.h:130-131`, `slSetFeatureLoaded`].
/// A switch waits without a time limit for driver compiles already running. Driver calls
/// cannot safely be cancelled; delaying this user-requested switch preserves device safety.
pub static RENDER_DEVICE_API_GATE: std::sync::RwLock<()> = std::sync::RwLock::new(());

/// NVForever Streamline integration: an unclaimed task output is destroyed under the gate.
/// async-task may discard a cancelled task's output after the compilation guard is released.
/// Taking the value transfers it to the render thread's cache, as upstream does.
#[derive(Debug)]
pub struct PipelineTaskOutput<T>(Option<T>);

impl<T> PipelineTaskOutput<T> {
    fn into_inner(mut self) -> T {
        self.0.take().unwrap()
    }
}

impl<T> Drop for PipelineTaskOutput<T> {
    fn drop(&mut self) {
        if self.0.is_some() {
            let _gate = RENDER_DEVICE_API_GATE
                .read()
                .unwrap_or_else(PoisonError::into_inner);
            drop(self.0.take());
        }
    }
}

pub struct CachedPipeline {
    pub descriptor: PipelineDescriptor,
    pub state: CachedPipelineState,
}

/// State of a cached pipeline inserted into a [`PipelineCache`].
#[derive(Debug)]
pub enum CachedPipelineState {
    /// The pipeline GPU object is queued for creation.
    Queued,
    /// The pipeline GPU object is being created.
    // NVForever Streamline integration: protect cancelled task outputs too.
    Creating(Task<PipelineTaskOutput<Result<Pipeline, ShaderCacheError>>>),
    /// The pipeline GPU object was created successfully and is available (allocated on the GPU).
    Ok(Pipeline),
    /// An error occurred while trying to create the pipeline GPU object.
    Err(ShaderCacheError),
}

impl CachedPipelineState {
    /// Convenience method to "unwrap" a pipeline state into its underlying GPU object.
    ///
    /// # Returns
    ///
    /// The method returns the allocated pipeline GPU object.
    ///
    /// # Panics
    ///
    /// This method panics if the pipeline GPU object is not available, either because it is
    /// pending creation or because an error occurred while attempting to create GPU object.
    pub fn unwrap(&self) -> &Pipeline {
        match self {
            CachedPipelineState::Ok(pipeline) => pipeline,
            CachedPipelineState::Queued => {
                panic!("Pipeline has not been compiled yet. It is still in the 'Queued' state.")
            }
            CachedPipelineState::Creating(..) => {
                panic!("Pipeline has not been compiled yet. It is still in the 'Creating' state.")
            }
            CachedPipelineState::Err(err) => panic!("{}", err),
        }
    }
}

// The default webgpu `max_bind_groups` is 4. Most desktop GPUs support 8.
// Since the element size we used below is small, we inline 8 on the stack.
const BIND_GROUP_LAYOUTS_INLINE_CAPACITY: usize = 8;

type ImmediateSize = u32;
type LayoutCacheKey = (
    SmallVec<[BindGroupLayoutId; BIND_GROUP_LAYOUTS_INLINE_CAPACITY]>,
    ImmediateSize,
);

wgpu_wrapper! {
    struct WgpuPipelineLayout(PipelineLayout);

    struct WgpuShaderModule(ShaderModule);
}

#[derive(Default)]
struct LayoutCache {
    layouts: HashMap<LayoutCacheKey, Arc<WgpuPipelineLayout>>,
}

impl LayoutCache {
    fn get(
        &mut self,
        render_device: &RenderDevice,
        bind_group_layouts: &[BindGroupLayout],
        immediate_size: u32,
    ) -> Arc<WgpuPipelineLayout> {
        let bind_group_ids = bind_group_layouts.iter().map(BindGroupLayout::id).collect();
        self.layouts
            .entry((bind_group_ids, immediate_size))
            .or_insert_with_key(|(_, immediate_size)| {
                let bind_group_layouts = bind_group_layouts
                    .iter()
                    .map(BindGroupLayout::value)
                    .map(Some)
                    .collect::<SmallVec<[_; BIND_GROUP_LAYOUTS_INLINE_CAPACITY]>>();
                Arc::new(WgpuPipelineLayout::new(
                    render_device.create_pipeline_layout(&PipelineLayoutDescriptor {
                        bind_group_layouts: &bind_group_layouts,
                        immediate_size: *immediate_size,
                        ..default()
                    }),
                ))
            })
            .clone()
    }
}

fn load_module(
    render_device: &RenderDevice,
    shader_source: ShaderCacheSource,
    validate_shader: &ValidateShader,
) -> Result<WgpuShaderModule, ShaderCacheError> {
    // NVForever pipeline timing.
    let began = std::time::Instant::now();
    let made = load_module_inner(render_device, shader_source, validate_shader);
    nvf_timing::MODULES.with(|m| {
        let (t, n) = m.get();
        m.set((t + began.elapsed(), n + 1));
    });
    made
}

fn load_module_inner(
    render_device: &RenderDevice,
    shader_source: ShaderCacheSource,
    validate_shader: &ValidateShader,
) -> Result<WgpuShaderModule, ShaderCacheError> {
    let shader_source = match shader_source {
        #[cfg(feature = "shader_format_spirv")]
        ShaderCacheSource::SpirV(data) => wgpu::util::make_spirv(data),
        #[cfg(not(feature = "shader_format_spirv"))]
        ShaderCacheSource::SpirV(_) => {
            unimplemented!("Enable feature \"shader_format_spirv\" to use SPIR-V shaders")
        }
        ShaderCacheSource::Wgsl(src) => ShaderSource::Wgsl(Cow::Owned(src)),
    };
    let module_descriptor = ShaderModuleDescriptor {
        label: None,
        source: shader_source,
    };

    let scope = render_device
        .wgpu_device()
        .push_error_scope(wgpu::ErrorFilter::Validation);

    let shader_module = WgpuShaderModule::new(match validate_shader {
        ValidateShader::Enabled => {
            render_device.create_and_validate_shader_module(module_descriptor)
        }
        // SAFETY: we are interfacing with shader code, which may contain undefined behavior,
        // such as indexing out of bounds.
        // The checks required are prohibitively expensive and a poor default for game engines.
        ValidateShader::Disabled => unsafe {
            render_device.create_shader_module(module_descriptor)
        },
    });

    let error = scope.pop();

    // `now_or_never` will return Some if the future is ready and None otherwise.
    // On native platforms, wgpu will yield the error immediately while on wasm it may take longer since the browser APIs are asynchronous.
    // So to keep the complexity of the ShaderCache low, we will only catch this error early on native platforms,
    // and on wasm the error will be handled by wgpu and crash the application.
    if let Some(Some(wgpu::Error::Validation { description, .. })) =
        bevy_tasks::futures::now_or_never(error)
    {
        return Err(ShaderCacheError::CreateShaderModule(description));
    }

    Ok(shader_module)
}

type NvfShaderCache = ShaderCache<WgpuShaderModule, RenderDevice>;

/// NVForever modules off the lock (bevy_shader `ModuleStep`): make the modules `prepare`
/// left to this thread without the shader cache's lock (naga runs on every compile thread at
/// once; wgpu's error scopes are per thread with its `std` feature, this crate's Cargo.toml),
/// then wait for those another thread is making. All of this thread's are made before any
/// wait, so two threads never wait on each other.
fn nvf_modules(
    shader_cache: &Mutex<NvfShaderCache>,
    device: &RenderDevice,
    steps: Vec<Option<ModuleStep<WgpuShaderModule>>>,
) -> Result<Vec<Option<Arc<WgpuShaderModule>>>, ShaderCacheError> {
    let mut out: Vec<Option<Arc<WgpuShaderModule>>> = steps.iter().map(|_| None).collect();
    let mut failed = None;
    let mut waits = Vec::new();
    for (i, step) in steps.into_iter().enumerate() {
        match step {
            None => {}
            Some(ModuleStep::Ready(module)) => out[i] = Some(module),
            Some(ModuleStep::Build(build)) => {
                let made = load_module(device, build.source(), build.validate());
                let kept = shader_cache
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .finish(build, made);
                match kept {
                    Ok(module) => out[i] = Some(module),
                    Err(e) => failed = failed.or(Some(e)),
                }
            }
            Some(ModuleStep::Wait(slot)) => waits.push((i, slot)),
        }
    }
    for (i, slot) in waits {
        match slot.wait() {
            Ok(module) => out[i] = Some(module),
            Err(e) => failed = failed.or(Some(e)),
        }
    }
    failed.map_or(Ok(out), Err)
}

/// NVForever pipeline list: the modules a pipeline is built from, as the list compares them.
fn nvf_module_list<'a>(
    modules: impl IntoIterator<Item = &'a Arc<WgpuShaderModule>>,
) -> Vec<nvf_manifest::Module> {
    modules
        .into_iter()
        .map(|m| m.clone() as nvf_manifest::Module)
        .collect()
}

/// NVForever pipeline list: what a game pipeline's ask gives (`PipelineManifest::ask`).
enum NvfAsk {
    Ready(Pipeline),
    Build(Option<Claim>),
}

fn nvf_ask(
    manifest: Option<&PipelineManifest>,
    recipe: Option<Recipe>,
    modules: &[nvf_manifest::Module],
) -> NvfAsk {
    let Some(manifest) = manifest else {
        return NvfAsk::Build(None);
    };
    match recipe.and_then(|r| manifest.ask(r, modules)) {
        Some(Asked::Ready(pipeline)) => NvfAsk::Ready(pipeline),
        Some(Asked::Build(claim)) => NvfAsk::Build(Some(claim)),
        None => {
            manifest.note_unlisted();
            NvfAsk::Build(None)
        }
    }
}

#[derive(Default)]
struct BindGroupLayoutCache {
    bgls: HashMap<BindGroupLayoutDescriptor, BindGroupLayout>,
}

impl BindGroupLayoutCache {
    fn get(
        &mut self,
        render_device: &RenderDevice,
        descriptor: &BindGroupLayoutDescriptor,
    ) -> BindGroupLayout {
        match self.bgls.raw_entry_mut().from_key(descriptor) {
            RawEntryMut::Occupied(entry) => entry.into_mut(),
            RawEntryMut::Vacant(slot) => {
                let created = render_device
                    .create_bind_group_layout(descriptor.label.as_ref(), &descriptor.entries);
                slot.insert(descriptor.clone(), created).1
            }
        }
        .clone()
    }
}

/// Cache for render and compute pipelines.
///
/// The cache stores existing render and compute pipelines allocated on the GPU, as well as
/// pending creation. Pipelines inserted into the cache are identified by a unique ID, which
/// can be used to retrieve the actual GPU object once it's ready. The creation of the GPU
/// pipeline object is deferred to the [`RenderSystems::Render`] step, just before the render
/// graph starts being processed, as this requires access to the GPU.
///
/// Note that the cache does not perform automatic deduplication of identical pipelines. It is
/// up to the user not to insert the same pipeline twice to avoid wasting GPU resources.
///
/// [`RenderSystems::Render`]: crate::RenderSystems::Render
#[derive(Resource)]
pub struct PipelineCache {
    layout_cache: Arc<Mutex<LayoutCache>>,
    bindgroup_layout_cache: Arc<Mutex<BindGroupLayoutCache>>,
    shader_cache: Arc<Mutex<ShaderCache<WgpuShaderModule, RenderDevice>>>,
    device: RenderDevice,
    pipelines: Vec<CachedPipeline>,
    waiting_pipelines: HashSet<CachedPipelineId>,
    new_pipelines: Mutex<Vec<CachedPipeline>>,
    global_shader_defs: Vec<ShaderDefVal>,
    /// If `true`, disables asynchronous pipeline compilation.
    /// This has no effect on macOS, wasm, or without the `multi_threaded` feature.
    pub(crate) synchronous_pipeline_compilation: bool,
    /// If `true`, the shader cache needs to be repopulated from the main world's `Assets<Shader>`.
    needs_shader_reload: bool,
    /// NVForever persistent pipeline cache: the driver's pipeline cache every pipeline is
    /// created with, which nvforever's `pipeline_disk_cache.rs` loads from the disk and saves.
    /// `None` (the default) creates them without one, as upstream does.
    wgpu_pipeline_cache: Option<wgpu::PipelineCache>,
    /// NVForever pipeline list (`nvf_manifest.rs`): `None` (the default) keeps none.
    nvf_manifest: Option<Arc<PipelineManifest>>,
}

impl PipelineCache {
    /// Returns an iterator over the pipelines in the pipeline cache.
    pub fn pipelines(&self) -> impl Iterator<Item = &CachedPipeline> {
        self.pipelines.iter()
    }

    /// Returns a iterator of the IDs of all currently waiting pipelines.
    pub fn waiting_pipelines(&self) -> impl Iterator<Item = CachedPipelineId> + '_ {
        self.waiting_pipelines.iter().copied()
    }

    /// Create a new pipeline cache associated with the given render device.
    pub fn new(device: RenderDevice, synchronous_pipeline_compilation: bool) -> Self {
        let mut global_shader_defs = Vec::new();
        #[cfg(all(feature = "webgl", target_arch = "wasm32", not(feature = "webgpu")))]
        {
            global_shader_defs.push("NO_ARRAY_TEXTURES_SUPPORT".into());
            global_shader_defs.push("NO_CUBE_ARRAY_TEXTURES_SUPPORT".into());
            global_shader_defs.push("SIXTEEN_BYTE_ALIGNMENT".into());
        }

        if cfg!(target_abi = "sim") {
            global_shader_defs.push("NO_CUBE_ARRAY_TEXTURES_SUPPORT".into());
        }

        let available_storage_buffer_bindings =
            device.limits().max_storage_buffers_per_shader_stage;
        global_shader_defs.push(ShaderDefVal::UInt(
            "AVAILABLE_STORAGE_BUFFER_BINDINGS".into(),
            available_storage_buffer_bindings,
        ));
        // wesl condcomp flags are booleans, so bake the comparisons in.
        global_shader_defs.push(ShaderDefVal::Bool(
            "AVAILABLE_STORAGE_BUFFER_BINDINGS__GE_3".into(),
            available_storage_buffer_bindings >= 3,
        ));
        global_shader_defs.push(ShaderDefVal::Bool(
            "AVAILABLE_STORAGE_BUFFER_BINDINGS__GE_6".into(),
            available_storage_buffer_bindings >= 6,
        ));

        Self {
            shader_cache: Arc::new(Mutex::new(ShaderCache::new(device.clone(), load_module))),
            device,
            layout_cache: default(),
            bindgroup_layout_cache: default(),
            waiting_pipelines: default(),
            new_pipelines: default(),
            pipelines: default(),
            global_shader_defs,
            synchronous_pipeline_compilation,
            needs_shader_reload: true,
            wgpu_pipeline_cache: None,
            nvf_manifest: None,
        }
    }

    /// NVForever pipeline list: record the pipelines asked for from now on in this list and
    /// build its pipelines in the background (`nvf_manifest.rs`; `None`: neither).
    pub fn set_pipeline_manifest(&mut self, manifest: Option<Arc<PipelineManifest>>) {
        self.nvf_manifest = manifest;
    }

    /// NVForever persistent pipeline cache: create the pipelines asked for from now on with
    /// this driver pipeline cache (the device needs `Features::PIPELINE_CACHE`).
    pub fn set_wgpu_pipeline_cache(&mut self, cache: Option<wgpu::PipelineCache>) {
        self.wgpu_pipeline_cache = cache;
    }

    /// NVForever composed shaders: compose Wesl shaders through this store from now on
    /// (bevy_shader `nvf_composed.rs`; `None`: compose every time, as upstream).
    pub fn set_composed_store(
        &mut self,
        store: Option<Arc<bevy_shader::nvf_composed::ComposedStore>>,
    ) {
        self.shader_cache
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .set_composed_store(store);
    }

    /// NVForever persistent pipeline cache: the driver pipeline cache in use, if any.
    pub fn wgpu_pipeline_cache(&self) -> Option<&wgpu::PipelineCache> {
        self.wgpu_pipeline_cache.as_ref()
    }

    /// Get the state of a cached render pipeline.
    ///
    /// See [`PipelineCache::queue_render_pipeline()`].
    #[inline]
    pub fn get_render_pipeline_state(&self, id: CachedRenderPipelineId) -> &CachedPipelineState {
        // If the pipeline id isn't in `pipelines`, it's queued in `new_pipelines`
        self.pipelines
            .get(id.id())
            .map_or(&CachedPipelineState::Queued, |pipeline| &pipeline.state)
    }

    /// Get the state of a cached compute pipeline.
    ///
    /// See [`PipelineCache::queue_compute_pipeline()`].
    #[inline]
    pub fn get_compute_pipeline_state(&self, id: CachedComputePipelineId) -> &CachedPipelineState {
        // If the pipeline id isn't in `pipelines`, it's queued in `new_pipelines`
        self.pipelines
            .get(id.id())
            .map_or(&CachedPipelineState::Queued, |pipeline| &pipeline.state)
    }

    /// Get the render pipeline descriptor a cached render pipeline was inserted from.
    ///
    /// See [`PipelineCache::queue_render_pipeline()`].
    ///
    /// **Note**: Be careful calling this method. It will panic if called with a pipeline that
    /// has been queued but has not yet been processed by [`PipelineCache::process_queue()`].
    #[inline]
    pub fn get_render_pipeline_descriptor(
        &self,
        id: CachedRenderPipelineId,
    ) -> &RenderPipelineDescriptor {
        match &self.pipelines[id.id()].descriptor {
            PipelineDescriptor::RenderPipelineDescriptor(descriptor) => descriptor,
            PipelineDescriptor::ComputePipelineDescriptor(_)
            | PipelineDescriptor::MeshPipelineDescriptor(_) => unreachable!(),
        }
    }

    /// Get the compute pipeline descriptor a cached render pipeline was inserted from.
    ///
    /// See [`PipelineCache::queue_compute_pipeline()`].
    ///
    /// **Note**: Be careful calling this method. It will panic if called with a pipeline that
    /// has been queued but has not yet been processed by [`PipelineCache::process_queue()`].
    #[inline]
    pub fn get_compute_pipeline_descriptor(
        &self,
        id: CachedComputePipelineId,
    ) -> &ComputePipelineDescriptor {
        match &self.pipelines[id.id()].descriptor {
            PipelineDescriptor::RenderPipelineDescriptor(_)
            | PipelineDescriptor::MeshPipelineDescriptor(_) => unreachable!(),
            PipelineDescriptor::ComputePipelineDescriptor(descriptor) => descriptor,
        }
    }

    /// Get the mesh pipeline descriptor a cached mesh pipeline was inserted from.
    ///
    /// See [`PipelineCache::queue_mesh_pipeline()`].
    ///
    /// **Note**: Be careful calling this method. It will panic if called with a pipeline that
    /// has been queued but has not yet been processed by [`PipelineCache::process_queue()`].
    #[inline]
    pub fn get_mesh_pipeline_descriptor(
        &self,
        id: CachedRenderPipelineId,
    ) -> &MeshPipelineDescriptor {
        match &self.pipelines[id.id()].descriptor {
            PipelineDescriptor::RenderPipelineDescriptor(_)
            | PipelineDescriptor::ComputePipelineDescriptor(_) => unreachable!(),
            PipelineDescriptor::MeshPipelineDescriptor(descriptor) => descriptor,
        }
    }

    /// Try to retrieve a render pipeline GPU object from a cached ID.
    ///
    /// # Returns
    ///
    /// This method returns a successfully created render pipeline if any, or `None` if the pipeline
    /// was not created yet or if there was an error during creation. You can check the actual creation
    /// state with [`PipelineCache::get_render_pipeline_state()`].
    #[inline]
    pub fn get_render_pipeline(&self, id: CachedRenderPipelineId) -> Option<&RenderPipeline> {
        if let CachedPipelineState::Ok(
            Pipeline::RenderPipeline(pipeline) | Pipeline::MeshPipeline(pipeline),
        ) = &self.pipelines.get(id.id())?.state
        {
            Some(pipeline)
        } else {
            None
        }
    }

    /// Wait for a render pipeline to finish compiling.
    #[inline]
    pub fn block_on_render_pipeline(&mut self, id: CachedRenderPipelineId) {
        if self.pipelines.len() <= id.id() {
            self.process_queue();
        }

        let state = &mut self.pipelines[id.id()].state;
        if let CachedPipelineState::Creating(task) = state {
            *state = match bevy_tasks::block_on(task).into_inner() {
                Ok(p) => CachedPipelineState::Ok(p),
                Err(e) => CachedPipelineState::Err(e),
            };
        }
    }

    /// Try to retrieve a compute pipeline GPU object from a cached ID.
    ///
    /// # Returns
    ///
    /// This method returns a successfully created compute pipeline if any, or `None` if the pipeline
    /// was not created yet or if there was an error during creation. You can check the actual creation
    /// state with [`PipelineCache::get_compute_pipeline_state()`].
    #[inline]
    pub fn get_compute_pipeline(&self, id: CachedComputePipelineId) -> Option<&ComputePipeline> {
        if let CachedPipelineState::Ok(Pipeline::ComputePipeline(pipeline)) =
            &self.pipelines.get(id.id())?.state
        {
            Some(pipeline)
        } else {
            None
        }
    }

    /// Insert a render pipeline into the cache, and queue its creation.
    ///
    /// The pipeline is always inserted and queued for creation. There is no attempt to deduplicate it with
    /// an already cached pipeline.
    ///
    /// # Returns
    ///
    /// This method returns the unique render shader ID of the cached pipeline, which can be used to query
    /// the caching state with [`get_render_pipeline_state()`] and to retrieve the created GPU pipeline once
    /// it's ready with [`get_render_pipeline()`].
    ///
    /// [`get_render_pipeline_state()`]: PipelineCache::get_render_pipeline_state
    /// [`get_render_pipeline()`]: PipelineCache::get_render_pipeline
    pub fn queue_render_pipeline(
        &self,
        descriptor: RenderPipelineDescriptor,
    ) -> CachedRenderPipelineId {
        let mut new_pipelines = self
            .new_pipelines
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let id = CachedRenderPipelineId::new(self.pipelines.len() + new_pipelines.len());
        new_pipelines.push(CachedPipeline {
            descriptor: PipelineDescriptor::RenderPipelineDescriptor(Box::new(descriptor)),
            state: CachedPipelineState::Queued,
        });
        id
    }

    /// Insert a compute pipeline into the cache, and queue its creation.
    ///
    /// The pipeline is always inserted and queued for creation. There is no attempt to deduplicate it with
    /// an already cached pipeline.
    ///
    /// # Returns
    ///
    /// This method returns the unique compute shader ID of the cached pipeline, which can be used to query
    /// the caching state with [`get_compute_pipeline_state()`] and to retrieve the created GPU pipeline once
    /// it's ready with [`get_compute_pipeline()`].
    ///
    /// [`get_compute_pipeline_state()`]: PipelineCache::get_compute_pipeline_state
    /// [`get_compute_pipeline()`]: PipelineCache::get_compute_pipeline
    pub fn queue_compute_pipeline(
        &self,
        descriptor: ComputePipelineDescriptor,
    ) -> CachedComputePipelineId {
        let mut new_pipelines = self
            .new_pipelines
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let id = CachedComputePipelineId::new(self.pipelines.len() + new_pipelines.len());
        new_pipelines.push(CachedPipeline {
            descriptor: PipelineDescriptor::ComputePipelineDescriptor(Box::new(descriptor)),
            state: CachedPipelineState::Queued,
        });
        id
    }

    /// Insert a mesh pipeline into the cache, and queue its creation.
    ///
    /// The pipeline is always inserted and queued for creation. There is no attempt to deduplicate it with
    /// an already cached pipeline.
    ///
    /// # Returns
    ///
    /// This method returns the unique mesh shader ID of the cached pipeline, which can be used to query
    /// the caching state with [`get_render_pipeline_state()`] and to retrieve the created GPU pipeline once
    /// it's ready with [`get_render_pipeline()`].
    ///
    /// [`get_render_pipeline_state()`]: PipelineCache::get_render_pipeline_state
    /// [`get_render_pipeline()`]: PipelineCache::get_render_pipeline
    pub fn queue_mesh_pipeline(
        &self,
        descriptor: MeshPipelineDescriptor,
    ) -> CachedRenderPipelineId {
        let mut new_pipelines = self
            .new_pipelines
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let id = CachedRenderPipelineId::new(self.pipelines.len() + new_pipelines.len());
        new_pipelines.push(CachedPipeline {
            descriptor: PipelineDescriptor::MeshPipelineDescriptor(Box::new(descriptor)),
            state: CachedPipelineState::Queued,
        });
        id
    }

    pub fn get_bind_group_layout(
        &self,
        bind_group_layout_descriptor: &BindGroupLayoutDescriptor,
    ) -> BindGroupLayout {
        self.bindgroup_layout_cache
            .lock()
            .unwrap()
            .get(&self.device, bind_group_layout_descriptor)
    }

    /// Inserts a [`Shader`] into this cache with the provided [`AssetId`].
    pub fn set_shader(&mut self, id: AssetId<Shader>, shader: Shader) {
        let mut shader_cache = self.shader_cache.lock().unwrap();
        let pipelines_to_queue = shader_cache.set_shader(id, shader);
        for cached_pipeline in pipelines_to_queue {
            self.pipelines[cached_pipeline].state = CachedPipelineState::Queued;
            self.waiting_pipelines.insert(cached_pipeline);
        }
    }

    /// Removes a [`Shader`] from this cache if it exists.
    pub fn remove_shader(&mut self, shader: AssetId<Shader>) {
        let mut shader_cache = self.shader_cache.lock().unwrap();
        let pipelines_to_queue = shader_cache.remove(shader);
        for cached_pipeline in pipelines_to_queue {
            self.pipelines[cached_pipeline].state = CachedPipelineState::Queued;
            self.waiting_pipelines.insert(cached_pipeline);
        }
    }

    fn start_create_render_pipeline(
        &mut self,
        id: CachedPipelineId,
        descriptor: RenderPipelineDescriptor,
    ) -> CachedPipelineState {
        let device = self.device.clone();
        let wgpu_pipeline_cache = self.wgpu_pipeline_cache.clone();
        let shader_cache = self.shader_cache.clone();
        let layout_cache = self.layout_cache.clone();
        let mut bindgroup_layout_cache = self.bindgroup_layout_cache.lock().unwrap();
        let bind_group_layout = descriptor
            .layout
            .iter()
            .map(|bind_group_layout_descriptor| {
                bindgroup_layout_cache.get(&self.device, bind_group_layout_descriptor)
            })
            .collect::<SmallVec<[_; BIND_GROUP_LAYOUTS_INLINE_CAPACITY]>>();

        let mut spans = nvf_timing::Spans::new();
        let manifest = self.nvf_manifest.clone();
        create_pipeline_task(
            async move {
                spans.start();
                let mut cache = shader_cache.lock().unwrap();
                let mut layout_cache = layout_cache.lock().unwrap();
                spans.locked();

                // NVForever modules off the lock: composed here, made below without the lock.
                let vertex_step = cache.prepare(
                    Some(id),
                    descriptor.vertex.shader.id(),
                    &descriptor.vertex.shader_defs,
                )?;
                let fragment_step = match &descriptor.fragment {
                    Some(fragment) => {
                        Some(cache.prepare(Some(id), fragment.shader.id(), &fragment.shader_defs)?)
                    }
                    None => None,
                };
                let recipe = manifest.as_ref().and_then(|_| {
                    Recipe::render(&descriptor, |s| cache.shader_path(s).map(str::to_owned))
                });

                let layout = if descriptor.layout.is_empty() && descriptor.immediate_size == 0 {
                    None
                } else {
                    Some(layout_cache.get(&device, &bind_group_layout, descriptor.immediate_size))
                };

                drop((cache, layout_cache));
                let mut modules =
                    nvf_modules(&shader_cache, &device, vec![Some(vertex_step), fragment_step])?
                        .into_iter();
                let vertex_module = modules.next().flatten().unwrap();
                let fragment_module = modules.next().flatten();
                spans.shaders_done();

                // NVForever pipeline list: built from the list already, or record it.
                let module_list =
                    nvf_module_list(core::iter::once(&vertex_module).chain(&fragment_module));
                let claim = match nvf_ask(manifest.as_deref(), recipe, &module_list) {
                    NvfAsk::Ready(pipeline) => {
                        spans.log(descriptor.label.as_deref());
                        return Ok(pipeline);
                    }
                    NvfAsk::Build(claim) => claim,
                };

                let vertex_buffer_layouts = descriptor
                    .vertex
                    .buffers
                    .iter()
                    .map(|layout| {
                        Some(RawVertexBufferLayout {
                            array_stride: layout.array_stride,
                            attributes: &layout.attributes,
                            step_mode: layout.step_mode,
                        })
                    })
                    .collect::<Vec<_>>();

                let fragment_data = descriptor.fragment.as_ref().map(|fragment| {
                    (
                        fragment_module.unwrap(),
                        fragment.entry_point.as_deref(),
                        fragment.targets.as_slice(),
                        fragment
                            .constants
                            .iter()
                            .map(|(k, v)| (k.as_ref(), *v))
                            .collect::<Vec<_>>(),
                    )
                });

                let vertex_constants: Vec<(&str, f64)> = descriptor
                    .vertex
                    .constants
                    .iter()
                    .map(|(k, v)| (k.as_ref(), *v))
                    .collect();

                let descriptor = RawRenderPipelineDescriptor {
                    multiview_mask: None,
                    depth_stencil: descriptor.depth_stencil.clone(),
                    label: descriptor.label.as_deref(),
                    layout: layout.as_ref().map(|layout| -> &PipelineLayout { layout }),
                    multisample: descriptor.multisample,
                    primitive: descriptor.primitive,
                    vertex: RawVertexState {
                        buffers: &vertex_buffer_layouts,
                        entry_point: descriptor.vertex.entry_point.as_deref(),
                        module: &vertex_module,
                        compilation_options: PipelineCompilationOptions {
                            constants: &vertex_constants,
                            zero_initialize_workgroup_memory: descriptor
                                .zero_initialize_workgroup_memory,
                        },
                    },
                    fragment: fragment_data.as_ref().map(
                        |(module, entry_point, targets, constants)| RawFragmentState {
                            entry_point: entry_point.as_deref(),
                            module,
                            targets,
                            compilation_options: PipelineCompilationOptions {
                                constants,
                                zero_initialize_workgroup_memory: descriptor
                                    .zero_initialize_workgroup_memory,
                            },
                        },
                    ),
                    cache: wgpu_pipeline_cache.as_ref(),
                };

                let made = Pipeline::RenderPipeline(device.create_render_pipeline(&descriptor));
                spans.log(descriptor.label);
                if let Some(claim) = claim {
                    claim.built(&made, module_list);
                }
                Ok(made)
            },
            self.synchronous_pipeline_compilation,
        )
    }

    fn start_create_compute_pipeline(
        &mut self,
        id: CachedPipelineId,
        descriptor: ComputePipelineDescriptor,
    ) -> CachedPipelineState {
        let device = self.device.clone();
        let wgpu_pipeline_cache = self.wgpu_pipeline_cache.clone();
        let shader_cache = self.shader_cache.clone();
        let layout_cache = self.layout_cache.clone();
        let mut bindgroup_layout_cache = self.bindgroup_layout_cache.lock().unwrap();
        let bind_group_layout = descriptor
            .layout
            .iter()
            .map(|bind_group_layout_descriptor| {
                bindgroup_layout_cache.get(&self.device, bind_group_layout_descriptor)
            })
            .collect::<SmallVec<[_; BIND_GROUP_LAYOUTS_INLINE_CAPACITY]>>();

        let mut spans = nvf_timing::Spans::new();
        let manifest = self.nvf_manifest.clone();
        create_pipeline_task(
            async move {
                spans.start();
                let mut cache = shader_cache.lock().unwrap();
                let mut layout_cache = layout_cache.lock().unwrap();
                spans.locked();

                // NVForever modules off the lock: composed here, made below without the lock.
                let step = cache.prepare(Some(id), descriptor.shader.id(), &descriptor.shader_defs)?;
                let recipe = manifest.as_ref().and_then(|_| {
                    Recipe::compute(&descriptor, |s| cache.shader_path(s).map(str::to_owned))
                });

                let layout = if descriptor.layout.is_empty() && descriptor.immediate_size == 0 {
                    None
                } else {
                    Some(layout_cache.get(&device, &bind_group_layout, descriptor.immediate_size))
                };

                drop((cache, layout_cache));
                let compute_module = nvf_modules(&shader_cache, &device, vec![Some(step)])?
                    .pop()
                    .flatten()
                    .unwrap();
                spans.shaders_done();

                // NVForever pipeline list: built from the list already, or record it.
                let module_list = nvf_module_list([&compute_module]);
                let claim = match nvf_ask(manifest.as_deref(), recipe, &module_list) {
                    NvfAsk::Ready(pipeline) => {
                        spans.log(descriptor.label.as_deref());
                        return Ok(pipeline);
                    }
                    NvfAsk::Build(claim) => claim,
                };

                let constants: Vec<(&str, f64)> = descriptor
                    .constants
                    .iter()
                    .map(|(k, v)| (k.as_ref(), *v))
                    .collect();
                let descriptor = RawComputePipelineDescriptor {
                    label: descriptor.label.as_deref(),
                    layout: layout.as_ref().map(|layout| -> &PipelineLayout { layout }),
                    module: &compute_module,
                    entry_point: descriptor.entry_point.as_deref(),
                    compilation_options: PipelineCompilationOptions {
                        constants: &constants,
                        zero_initialize_workgroup_memory: descriptor
                            .zero_initialize_workgroup_memory,
                    },
                    cache: wgpu_pipeline_cache.as_ref(),
                };

                let made = Pipeline::ComputePipeline(device.create_compute_pipeline(&descriptor));
                spans.log(descriptor.label);
                if let Some(claim) = claim {
                    claim.built(&made, module_list);
                }
                Ok(made)
            },
            self.synchronous_pipeline_compilation,
        )
    }

    fn start_create_mesh_pipeline(
        &mut self,
        id: CachedPipelineId,
        descriptor: MeshPipelineDescriptor,
    ) -> CachedPipelineState {
        let device = self.device.clone();
        let wgpu_pipeline_cache = self.wgpu_pipeline_cache.clone();
        let shader_cache = self.shader_cache.clone();
        let layout_cache = self.layout_cache.clone();
        let mut bindgroup_layout_cache = self.bindgroup_layout_cache.lock().unwrap();
        let bind_group_layout = descriptor
            .layout
            .iter()
            .map(|bind_group_layout_descriptor| {
                bindgroup_layout_cache.get(&self.device, bind_group_layout_descriptor)
            })
            .collect::<SmallVec<[_; BIND_GROUP_LAYOUTS_INLINE_CAPACITY]>>();

        let mut spans = nvf_timing::Spans::new();
        create_pipeline_task(
            async move {
                spans.start();
                let mut cache = shader_cache.lock().unwrap();
                let mut layout_cache = layout_cache.lock().unwrap();
                spans.locked();

                // NVForever modules off the lock: composed here, made below without the lock.
                let mesh_step = cache.prepare(
                    Some(id),
                    descriptor.mesh.shader.id(),
                    &descriptor.mesh.shader_defs,
                )?;
                let fragment_step = match &descriptor.fragment {
                    Some(fragment) => {
                        Some(cache.prepare(Some(id), fragment.shader.id(), &fragment.shader_defs)?)
                    }
                    None => None,
                };
                let task_step = match &descriptor.task {
                    Some(task) => Some(cache.prepare(Some(id), task.shader.id(), &task.shader_defs)?),
                    None => None,
                };

                let layout = if descriptor.layout.is_empty() && descriptor.immediate_size == 0 {
                    None
                } else {
                    Some(layout_cache.get(&device, &bind_group_layout, descriptor.immediate_size))
                };

                drop((cache, layout_cache));
                let mut modules = nvf_modules(
                    &shader_cache,
                    &device,
                    vec![Some(mesh_step), fragment_step, task_step],
                )?
                .into_iter();
                let mesh_module = modules.next().flatten().unwrap();
                let fragment_module = modules.next().flatten();
                let task_module = modules.next().flatten();
                spans.shaders_done();

                let fragment_data = descriptor.fragment.as_ref().map(|fragment| {
                    (
                        fragment_module.unwrap(),
                        fragment.entry_point.as_deref(),
                        fragment.targets.as_slice(),
                        fragment
                            .constants
                            .iter()
                            .map(|(k, v)| (k.as_ref(), *v))
                            .collect::<Vec<_>>(),
                    )
                });

                let mesh_constants: Vec<(&str, f64)> = descriptor
                    .mesh
                    .constants
                    .iter()
                    .map(|(k, v)| (k.as_ref(), *v))
                    .collect();

                let task_data = descriptor.task.as_ref().map(|task| {
                    (
                        task_module.unwrap(),
                        task.entry_point.as_deref(),
                        task.constants
                            .iter()
                            .map(|(k, v)| (k.as_ref(), *v))
                            .collect::<Vec<_>>(),
                    )
                });

                let descriptor = RawMeshPipelineDescriptor {
                    multiview: None,
                    depth_stencil: descriptor.depth_stencil.clone(),
                    label: descriptor.label.as_deref(),
                    layout: layout.as_ref().map(|layout| -> &PipelineLayout { layout }),
                    multisample: descriptor.multisample,
                    primitive: descriptor.primitive,
                    task: task_data
                        .as_ref()
                        .map(|(module, entry_point, constants)| RawTaskState {
                            entry_point: entry_point.as_deref(),
                            module,
                            compilation_options: PipelineCompilationOptions {
                                constants,
                                zero_initialize_workgroup_memory: descriptor
                                    .zero_initialize_workgroup_memory,
                            },
                        }),
                    mesh: RawMeshState {
                        module: &mesh_module,
                        entry_point: descriptor.mesh.entry_point.as_deref(),
                        compilation_options: PipelineCompilationOptions {
                            constants: &mesh_constants,
                            zero_initialize_workgroup_memory: descriptor
                                .zero_initialize_workgroup_memory,
                        },
                    },
                    fragment: fragment_data.as_ref().map(
                        |(module, entry_point, targets, constants)| RawFragmentState {
                            entry_point: entry_point.as_deref(),
                            module,
                            targets,
                            compilation_options: PipelineCompilationOptions {
                                constants,
                                zero_initialize_workgroup_memory: descriptor
                                    .zero_initialize_workgroup_memory,
                            },
                        },
                    ),
                    cache: wgpu_pipeline_cache.as_ref(),
                };

                let made = device.create_mesh_pipeline(&descriptor);
                spans.log(descriptor.label);
                Ok(Pipeline::MeshPipeline(made))
            },
            self.synchronous_pipeline_compilation,
        )
    }
    /// Process the pipeline queue and create all pending pipelines if possible.
    ///
    /// This is generally called automatically during the [`RenderSystems::Render`] step, but can
    /// be called manually to force creation at a different time.
    ///
    /// [`RenderSystems::Render`]: crate::RenderSystems::Render
    pub fn process_queue(&mut self) {
        self.nvf_feed_manifest();
        let mut waiting_pipelines = mem::take(&mut self.waiting_pipelines);
        let mut pipelines = mem::take(&mut self.pipelines);

        {
            let mut new_pipelines = self
                .new_pipelines
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            for new_pipeline in new_pipelines.drain(..) {
                let id = pipelines.len();
                pipelines.push(new_pipeline);
                waiting_pipelines.insert(id);
            }
        }

        for id in waiting_pipelines {
            self.process_pipeline(&mut pipelines[id], id);
        }

        self.pipelines = pipelines;
    }

    fn process_pipeline(&mut self, cached_pipeline: &mut CachedPipeline, id: usize) {
        match &mut cached_pipeline.state {
            CachedPipelineState::Queued => {
                cached_pipeline.state = match &cached_pipeline.descriptor {
                    PipelineDescriptor::RenderPipelineDescriptor(descriptor) => {
                        self.start_create_render_pipeline(id, *descriptor.clone())
                    }
                    PipelineDescriptor::ComputePipelineDescriptor(descriptor) => {
                        self.start_create_compute_pipeline(id, *descriptor.clone())
                    }
                    PipelineDescriptor::MeshPipelineDescriptor(descriptor) => {
                        self.start_create_mesh_pipeline(id, *descriptor.clone())
                    }
                };
            }

            CachedPipelineState::Creating(task) => match bevy_tasks::futures::check_ready(task)
                .map(PipelineTaskOutput::into_inner)
            {
                Some(Ok(pipeline)) => {
                    cached_pipeline.state = CachedPipelineState::Ok(pipeline);
                    return;
                }
                Some(Err(err)) => cached_pipeline.state = CachedPipelineState::Err(err),
                _ => (),
            },

            CachedPipelineState::Err(err) => match err {
                // Retry
                ShaderCacheError::ShaderNotLoaded(_)
                | ShaderCacheError::ShaderImportNotYetAvailable => {
                    bevy_log::debug!("retry processing pipeline {id}: {err}");
                    cached_pipeline.state = CachedPipelineState::Queued;
                }

                // Shader could not be processed ... retrying won't help
                ShaderCacheError::ProcessShaderError(error_detail) => {
                    let error_detail = error_detail.clone();
                    if std::env::var("VERBOSE_SHADER_ERROR")
                        .is_ok_and(|v| !(v.is_empty() || v == "0" || v == "false"))
                    {
                        error!("{}", pipeline_error_context(cached_pipeline));
                    }
                    error!("failed to process shader error:\n{}", error_detail);
                    return;
                }
                ShaderCacheError::CreateShaderModule(description) => {
                    error!("failed to create shader module: {}", description);
                    return;
                }
            },

            CachedPipelineState::Ok(_) => return,
        }

        // Retry
        self.waiting_pipelines.insert(id);
    }

    /// NVForever pipeline list: hand the list's next pipelines whose shaders have loaded to
    /// its pool, keeping at most two a thread in flight (`nvf_manifest.rs`).
    fn nvf_feed_manifest(&mut self) {
        let Some(manifest) = self.nvf_manifest.clone() else {
            return;
        };
        if self.synchronous_pipeline_compilation || !manifest.loaded() {
            return;
        }
        let Some(pool) = nvf_list_pool() else { return };
        let cap = pool.thread_num() * 2;
        let in_flight = manifest.in_flight.load(core::sync::atomic::Ordering::Relaxed) as usize;
        if in_flight >= cap || manifest.queued() == 0 {
            return;
        }
        // Not while a compile thread composes under the lock: the render thread does not wait.
        let Ok(cache) = self.shader_cache.try_lock() else {
            return;
        };
        let next = manifest.next(cap - in_flight, cache.shaders_set(), |path| {
            cache.shader_by_path(path).is_some()
        });
        drop(cache);
        for entry in next {
            manifest
                .in_flight
                .fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            let job = NvfListJob {
                device: self.device.clone(),
                shader_cache: self.shader_cache.clone(),
                layout_cache: self.layout_cache.clone(),
                bindgroup_layout_cache: self.bindgroup_layout_cache.clone(),
                wgpu_pipeline_cache: self.wgpu_pipeline_cache.clone(),
                manifest: manifest.clone(),
            };
            pool.spawn(async move {
                let _gate = RENDER_DEVICE_API_GATE
                    .read()
                    .unwrap_or_else(PoisonError::into_inner);
                job.build(entry);
                job.manifest
                    .in_flight
                    .fetch_sub(1, core::sync::atomic::Ordering::Relaxed);
            })
            .detach();
        }
    }

    pub(crate) fn process_pipeline_queue_system(mut cache: ResMut<Self>) {
        cache.process_queue();
    }

    pub(crate) fn extract_shaders(
        mut cache: ResMut<Self>,
        shaders: Extract<Res<Assets<Shader>>>,
        mut events: Extract<MessageReader<AssetEvent<Shader>>>,
    ) {
        if cache.needs_shader_reload {
            cache.needs_shader_reload = false;
            for (id, shader) in shaders.iter() {
                let mut shader = shader.clone();
                shader
                    .shader_defs
                    .splice(0..0, cache.global_shader_defs.iter().cloned());
                cache.set_shader(id, shader);
            }
            // Drain events so we don't double-process shaders we just loaded.
            for _ in events.read() {}
            return;
        }

        for event in events.read() {
            #[expect(
                clippy::match_same_arms,
                reason = "LoadedWithDependencies is marked as a TODO, so it's likely this will no longer lint soon."
            )]
            match event {
                // PERF: Instead of blocking waiting for the shader cache lock, try again next frame if the lock is currently held
                AssetEvent::Added { id } | AssetEvent::Modified { id } => {
                    if let Some(shader) = shaders.get(*id) {
                        let mut shader = shader.clone();
                        shader
                            .shader_defs
                            .splice(0..0, cache.global_shader_defs.iter().cloned());

                        cache.set_shader(*id, shader);
                    }
                }
                AssetEvent::Removed { id } => cache.remove_shader(*id),
                AssetEvent::Unused { .. } => {}
                AssetEvent::LoadedWithDependencies { .. } => {
                    // TODO: handle this
                }
            }
        }
    }
}

/// NVForever pipeline list: one of the list's pipelines, built on the list's pool.
struct NvfListJob {
    device: RenderDevice,
    shader_cache: Arc<Mutex<NvfShaderCache>>,
    layout_cache: Arc<Mutex<LayoutCache>>,
    bindgroup_layout_cache: Arc<Mutex<BindGroupLayoutCache>>,
    wgpu_pipeline_cache: Option<wgpu::PipelineCache>,
    manifest: Arc<PipelineManifest>,
}

impl NvfListJob {
    /// Build it unless the game has; a shader not loaded yet sends it back to wait, and one
    /// that fails (to compose, or to create: it runs inside an error scope, so a stale entry
    /// is no render error) drops it from the list.
    fn build(&self, entry: Arc<nvf_manifest::Entry>) {
        let recipe = &entry.recipe;
        let steps = {
            let mut cache = self.shader_cache.lock().unwrap_or_else(PoisonError::into_inner);
            let mut steps = Vec::new();
            for stage in recipe.stages() {
                // On an early return the stages prepared so far (`steps`) are dropped while
                // the lock is still held: a `ModuleStep::Build` among them fails its slot
                // before any other thread can be handed that slot to wait on.
                let Some(id) = cache.shader_by_path(&stage.shader) else {
                    drop(steps);
                    drop(cache);
                    self.manifest.retry(entry);
                    return;
                };
                match cache.prepare(None, id, &stage.defs) {
                    Ok(step) => steps.push(Some(step)),
                    Err(
                        ShaderCacheError::ShaderNotLoaded(_)
                        | ShaderCacheError::ShaderImportNotYetAvailable,
                    ) => {
                        drop(steps);
                        drop(cache);
                        self.manifest.retry(entry);
                        return;
                    }
                    Err(_) => {
                        drop(steps);
                        drop(cache);
                        self.fail(&entry);
                        return;
                    }
                }
            }
            steps
        };
        let modules = match nvf_modules(&self.shader_cache, &self.device, steps) {
            Ok(modules) => modules,
            // Another thread's maker stopped before making a module this one waited for.
            Err(ShaderCacheError::ShaderNotLoaded(_) | ShaderCacheError::ShaderImportNotYetAvailable) => {
                self.manifest.retry(entry);
                return;
            }
            Err(_) => {
                self.fail(&entry);
                return;
            }
        };
        let modules: Vec<Arc<WgpuShaderModule>> = modules.into_iter().flatten().collect();
        // The game's own task may have taken it meanwhile.
        let Some(claim) = entry.try_claim() else {
            return;
        };
        let (Recipe::Render { immediate_size, .. } | Recipe::Compute { immediate_size, .. }) =
            recipe;
        let bind_group_layouts = {
            let mut cache = self
                .bindgroup_layout_cache
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            recipe
                .layout_descriptors()
                .iter()
                .map(|d| cache.get(&self.device, d))
                .collect::<SmallVec<[_; BIND_GROUP_LAYOUTS_INLINE_CAPACITY]>>()
        };
        let layout = (!bind_group_layouts.is_empty() || *immediate_size != 0).then(|| {
            self.layout_cache
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .get(&self.device, &bind_group_layouts, *immediate_size)
        });
        let layout = layout.as_ref().map(|layout| -> &PipelineLayout { layout });
        let cache = self.wgpu_pipeline_cache.as_ref();

        let scope = self
            .device
            .wgpu_device()
            .push_error_scope(wgpu::ErrorFilter::Validation);
        let made = match recipe {
            Recipe::Render {
                label,
                vertex,
                buffers,
                primitive,
                depth_stencil,
                multisample,
                fragment,
                zero_init,
                ..
            } => {
                let buffers = buffers
                    .iter()
                    .map(|b| {
                        Some(RawVertexBufferLayout {
                            array_stride: b.stride,
                            attributes: &b.attributes,
                            step_mode: b.step,
                        })
                    })
                    .collect::<Vec<_>>();
                fn constants(stage: &nvf_manifest::Stage) -> Vec<(&str, f64)> {
                    stage
                        .constants
                        .iter()
                        .map(|(k, v)| (k.as_str(), *v))
                        .collect()
                }
                let vertex_constants = constants(vertex);
                let fragment_constants = fragment.as_ref().map(|(f, _)| constants(f));
                let descriptor = RawRenderPipelineDescriptor {
                    multiview_mask: None,
                    depth_stencil: depth_stencil.clone(),
                    label: label.as_deref(),
                    layout,
                    multisample: *multisample,
                    primitive: *primitive,
                    vertex: RawVertexState {
                        buffers: &buffers,
                        entry_point: vertex.entry.as_deref(),
                        module: &modules[0],
                        compilation_options: PipelineCompilationOptions {
                            constants: &vertex_constants,
                            zero_initialize_workgroup_memory: *zero_init,
                        },
                    },
                    fragment: fragment.as_ref().zip(fragment_constants.as_ref()).map(
                        |((stage, targets), constants)| RawFragmentState {
                            entry_point: stage.entry.as_deref(),
                            module: &modules[1],
                            targets,
                            compilation_options: PipelineCompilationOptions {
                                constants,
                                zero_initialize_workgroup_memory: *zero_init,
                            },
                        },
                    ),
                    cache,
                };
                Pipeline::RenderPipeline(self.device.create_render_pipeline(&descriptor))
            }
            Recipe::Compute {
                label,
                stage,
                zero_init,
                ..
            } => {
                let constants = stage
                    .constants
                    .iter()
                    .map(|(k, v)| (k.as_str(), *v))
                    .collect::<Vec<_>>();
                let descriptor = RawComputePipelineDescriptor {
                    label: label.as_deref(),
                    layout,
                    module: &modules[0],
                    entry_point: stage.entry.as_deref(),
                    compilation_options: PipelineCompilationOptions {
                        constants: &constants,
                        zero_initialize_workgroup_memory: *zero_init,
                    },
                    cache,
                };
                Pipeline::ComputePipeline(self.device.create_compute_pipeline(&descriptor))
            }
        };
        let error = bevy_tasks::futures::now_or_never(scope.pop()).flatten();
        if let Some(error) = error {
            bevy_log::debug!("pipeline list: {:?} not built: {error}", recipe_label(recipe));
            claim.failed();
            self.manifest.note_replayed(false);
        } else {
            claim.built(&made, nvf_module_list(&modules));
            self.manifest.note_replayed(true);
        }
    }

    fn fail(&self, entry: &Arc<nvf_manifest::Entry>) {
        if let Some(claim) = entry.try_claim() {
            claim.failed();
            self.manifest.note_replayed(false);
        }
    }
}

fn recipe_label(recipe: &Recipe) -> Option<&str> {
    let (Recipe::Render { label, .. } | Recipe::Compute { label, .. }) = recipe;
    label.as_deref()
}

fn pipeline_error_context(cached_pipeline: &CachedPipeline) -> String {
    fn format(
        shader: &Handle<Shader>,
        entry: &Option<Cow<'static, str>>,
        shader_defs: &[ShaderDefVal],
    ) -> String {
        let source = match shader.path() {
            Some(path) => path.path().to_string_lossy().to_string(),
            None => String::new(),
        };
        let entry = match entry {
            Some(entry) => entry.to_string(),
            None => String::new(),
        };
        let shader_defs = shader_defs
            .iter()
            .flat_map(|def| match def {
                ShaderDefVal::Bool(k, v) if *v => Some(k.to_string()),
                ShaderDefVal::Int(k, v) => Some(format!("{k} = {v}")),
                ShaderDefVal::UInt(k, v) => Some(format!("{k} = {v}")),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(", ");
        format!("{source}:{entry}\nshader defs: {shader_defs}")
    }
    match &cached_pipeline.descriptor {
        PipelineDescriptor::RenderPipelineDescriptor(desc) => {
            let vert = &desc.vertex;
            let vert_str = format(&vert.shader, &vert.entry_point, &vert.shader_defs);
            let Some(frag) = desc.fragment.as_ref() else {
                return vert_str;
            };
            let frag_str = format(&frag.shader, &frag.entry_point, &frag.shader_defs);
            format!("vertex {vert_str}\nfragment {frag_str}")
        }
        PipelineDescriptor::ComputePipelineDescriptor(desc) => {
            format(&desc.shader, &desc.entry_point, &desc.shader_defs)
        }
        PipelineDescriptor::MeshPipelineDescriptor(desc) => {
            let mesh = &desc.mesh;
            let mesh_str = format(&mesh.shader, &mesh.entry_point, &mesh.shader_defs);

            let task = &desc.task;
            let task_str = task
                .as_ref()
                .map(|task| format(&task.shader, &task.entry_point, &task.shader_defs));

            let frag = &desc.fragment;
            let frag_str = frag
                .as_ref()
                .map(|frag| format(&frag.shader, &frag.entry_point, &frag.shader_defs));

            match (task_str, frag_str) {
                (None, None) => format!("mesh {mesh_str}"),
                (None, Some(frag_str)) => format!("mesh {mesh_str}\nfragment {frag_str}"),
                (Some(task_str), None) => format!("task {task_str}\nmesh {mesh_str}"),
                (Some(task_str), Some(frag_str)) => {
                    format!("task {task_str}\nmesh {mesh_str}\nfragment {frag_str}")
                }
            }
        }
    }
}

#[cfg(all(
    not(target_arch = "wasm32"),
    not(target_os = "macos"),
    feature = "multi_threaded"
))]
fn create_pipeline_task(
    task: impl Future<Output = Result<Pipeline, ShaderCacheError>> + Send + 'static,
    sync: bool,
) -> CachedPipelineState {
    // NVForever Streamline integration: the compilation futures below contain synchronous
    // driver work. Keep the guard inside the task, including while it waits to start.
    let task = async move {
        let _gate = RENDER_DEVICE_API_GATE
            .read()
            .unwrap_or_else(PoisonError::into_inner);
        PipelineTaskOutput(Some(bevy_tasks::block_on(task)))
    };
    if !sync {
        if let Some(pool) = nvf_compile_pool() {
            return CachedPipelineState::Creating(pool.spawn(task));
        }
        return CachedPipelineState::Creating(bevy_tasks::AsyncComputeTaskPool::get().spawn(task));
    }

    match bevy_tasks::block_on(task).into_inner() {
        Ok(pipeline) => CachedPipelineState::Ok(pipeline),
        Err(err) => CachedPipelineState::Err(err),
    }
}

/// NVForever compile pool: pipelines compile on a pool of their own, not on Bevy's async
/// compute pool. A pipeline the driver has not compiled before takes it 0.6-0.8 s of one
/// thread, and the async compute pool has 25 % of the logical cores, at most 4, shared with
/// the game's loading work: at a Novac start on a cold driver cache 200 pipelines waited 191 s
/// in all for a thread and the screen 5.7 s for them; with 12 threads 3.7 s [observed
/// 2026-11-19, `NVF_PLAY_TEST_SPIRV_SALT`, `NVF_PLAY_TEST_ASYNC_THREADS=12`]. The pool has a
/// quarter of the logical cores, from 2 to 8 threads; `NVF_PIPELINE_THREADS=<n>` sets it,
/// and 0 compiles on the async compute pool as upstream does. Its threads sleep when nothing
/// compiles; with the driver's cache warm a pipeline takes them a few milliseconds.
#[cfg(all(
    not(target_arch = "wasm32"),
    not(target_os = "macos"),
    feature = "multi_threaded"
))]
fn nvf_compile_pool() -> Option<&'static bevy_tasks::TaskPool> {
    static POOL: std::sync::OnceLock<Option<bevy_tasks::TaskPool>> = std::sync::OnceLock::new();
    POOL.get_or_init(|| {
        let cores = std::thread::available_parallelism().map_or(4, usize::from);
        let threads = nvf_compile_threads(
            std::env::var("NVF_PIPELINE_THREADS").ok().as_deref(),
            cores,
        );
        bevy_log::info!("pipeline compile pool: {threads} threads ({cores} logical cores)");
        (threads > 0).then(|| {
            bevy_tasks::TaskPoolBuilder::new()
                .num_threads(threads)
                .thread_name("Pipeline Compile Pool".into())
                .build()
        })
    })
    .as_ref()
}

/// NVForever compile pool: its threads for `NVF_PIPELINE_THREADS` and the logical cores.
fn nvf_compile_threads(setting: Option<&str>, cores: usize) -> usize {
    match setting.and_then(|v| v.trim().parse::<usize>().ok()) {
        Some(n) => n.min(64),
        None => (cores / 4).clamp(2, 8),
    }
}

/// NVForever pipeline list: the pool that builds the list's pipelines, half the compile
/// pool's default threads (at least one), apart from it so the game's own pipelines never
/// queue behind the list's (`nvf_manifest.rs`).
#[cfg(all(
    not(target_arch = "wasm32"),
    not(target_os = "macos"),
    feature = "multi_threaded"
))]
fn nvf_list_pool() -> Option<&'static bevy_tasks::TaskPool> {
    static POOL: std::sync::OnceLock<bevy_tasks::TaskPool> = std::sync::OnceLock::new();
    // `NVF_PIPELINE_THREADS=0` is upstream's path: no pool of the list's either (its builds
    // would otherwise make the game's tasks wait for them on the async compute pool).
    nvf_compile_pool()?;
    Some(POOL.get_or_init(|| {
        let cores = std::thread::available_parallelism().map_or(4, usize::from);
        let threads = (nvf_compile_threads(None, cores) / 2).max(1);
        bevy_log::info!("pipeline list pool: {threads} threads");
        bevy_tasks::TaskPoolBuilder::new()
            .num_threads(threads)
            .thread_name("Pipeline List Pool".into())
            .build()
    }))
}

#[cfg(not(all(
    not(target_arch = "wasm32"),
    not(target_os = "macos"),
    feature = "multi_threaded"
)))]
fn nvf_list_pool() -> Option<&'static bevy_tasks::TaskPool> {
    None
}

#[cfg(any(
    target_arch = "wasm32",
    target_os = "macos",
    not(feature = "multi_threaded")
))]
fn create_pipeline_task(
    task: impl Future<Output = Result<Pipeline, ShaderCacheError>> + Send + 'static,
    _sync: bool,
) -> CachedPipelineState {
    let _gate = RENDER_DEVICE_API_GATE
        .read()
        .unwrap_or_else(PoisonError::into_inner);
    match bevy_tasks::block_on(task) {
        Ok(pipeline) => CachedPipelineState::Ok(pipeline),
        Err(err) => CachedPipelineState::Err(err),
    }
}

// NVForever Streamline integration: no GPU or scheduler timing needed to check teardown.
#[cfg(test)]
mod task_output_tests {
    use super::{nvf_compile_threads, PipelineTaskOutput, RENDER_DEVICE_API_GATE};
    use core::sync::atomic::{AtomicBool, Ordering};

    struct DropProbe<'a>(&'a AtomicBool);

    impl Drop for DropProbe<'_> {
        fn drop(&mut self) {
            assert!(RENDER_DEVICE_API_GATE.try_write().is_err());
            self.0.store(true, Ordering::Relaxed);
        }
    }

    #[test]
    fn discarded_task_output_is_destroyed_under_the_gate() {
        let destroyed = AtomicBool::new(false);
        drop(PipelineTaskOutput(Some(DropProbe(&destroyed))));
        assert!(destroyed.load(Ordering::Relaxed));
        assert!(RENDER_DEVICE_API_GATE.try_write().is_ok());
    }

    // NVForever compile pool.
    #[test]
    fn the_compile_pool_has_a_quarter_of_the_cores_within_two_to_eight() {
        assert_eq!(nvf_compile_threads(None, 32), 8);
        assert_eq!(nvf_compile_threads(None, 64), 8);
        assert_eq!(nvf_compile_threads(None, 16), 4);
        assert_eq!(nvf_compile_threads(None, 4), 2);
        assert_eq!(nvf_compile_threads(Some("12"), 4), 12, "the setting wins");
        assert_eq!(nvf_compile_threads(Some("0"), 32), 0, "0: the async compute pool");
        assert_eq!(nvf_compile_threads(Some("many"), 32), 8, "not a number: the default");
    }
}
