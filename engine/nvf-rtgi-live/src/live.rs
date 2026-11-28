//! The scene on a device, the kernel's pipelines, and the points of a room solved with both, batch by batch; what they
//! wrote, read back.

use crate::accel::{scoped, supported, Accel};
use crate::tables::{PackedLight, Points, Tables, PLACE};
use nvf_rtgi::{LightState, Moments, Probe, V3};
use std::sync::{Arc, Mutex};
use wgpu::util::DeviceExt;

pub const WGSL: &str = include_str!("kernel.wgsl");
/// Rays of one dispatch at most, shadow rays not counted: a driver that is kept waiting longer gives the device up.
pub const MOST_RAYS_A_DISPATCH: u32 = 1 << 18;
/// Rays of one dispatch at most if each of a point's met a surface where most lamps reach, and all of them had a shadow
/// ray.
const MOST_WITH_SHADOWS: u32 = 1 << 24;
/// Words of the shader's `Params`, bytes of a batch's (a uniform's offset is a multiple of at most 256).
const PARAMS: usize = 16;
const SLOT: u64 = 256;
/// Bytes of a point's moments and counts, of its hits and backs, of a meeting and of a ray's answer.
const MOMENTS: u64 = 64;
const REACH: u64 = 8;
const MEETING: u64 = 32;
const ANSWER: u64 = 80;

/// What a solve writes, to a buffer ([`Live::out`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// Each point's moments and counts ([`probes_of`]).
    Moments,
    /// Each point's hits and backs ([`reach_of`]): whether it is valid, as [`Probe::valid`] has it. Nothing is lit and
    /// the light is not read.
    Reach,
    /// What each single ray met and brought ([`answers_of`]): for looking into differences.
    Replay,
    /// The solver's triangle test, point i against primitive i along direction i % rays ([`meetings_of`]).
    Meetings,
}

impl Target {
    fn entry(self) -> &'static str {
        match self {
            Target::Moments => "solve",
            Target::Reach => "reach",
            Target::Replay => "replay",
            Target::Meetings => "meetings",
        }
    }
}

/// The pipeline for a target. It is made on the device itself, by no cache and no list of pipelines.
pub struct Kernel {
    target: Target,
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::ComputePipeline,
}

impl Kernel {
    pub fn new(device: &wgpu::Device, target: Target) -> Result<Kernel, String> {
        supported(device)?;
        let buffer = |ty, size| wgpu::BindingType::Buffer { ty, has_dynamic_offset: size > 0, min_binding_size: wgpu::BufferSize::new(size) };
        let storage = |read_only| buffer(wgpu::BufferBindingType::Storage { read_only }, 0);
        let mut bindings: Vec<(u32, wgpu::BindingType)> = vec![(0, wgpu::BindingType::AccelerationStructure { vertex_return: false })];
        bindings.extend((1..7).map(|b| (b, storage(true))));
        bindings.push((7, buffer(wgpu::BufferBindingType::Uniform, 4 * PARAMS as u64)));
        bindings.push((8, storage(false)));
        let entries: Vec<_> = bindings.into_iter().map(|(binding, ty)| wgpu::BindGroupLayoutEntry { binding, visibility: wgpu::ShaderStages::COMPUTE, ty, count: None }).collect();
        let entry = target.entry();
        scoped(device, "the kernel", || {
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("rtgi_live"), source: wgpu::ShaderSource::Wgsl(WGSL.into()) });
            let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("rtgi_live"), entries: &entries });
            let whole = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("rtgi_live"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
            let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor { label: Some(entry), layout: Some(&whole), module: &module, entry_point: Some(entry), compilation_options: Default::default(), cache: None });
            Kernel { target, layout, pipeline }
        })
    }

    pub fn target(&self) -> Target {
        self.target
    }
}

/// A buffer of these words; never empty, which could not be bound.
fn held<T: bytemuck::NoUninit>(device: &wgpu::Device, label: &str, of: &[T], usage: wgpu::BufferUsages) -> wgpu::Buffer {
    let contents: &[u8] = if of.is_empty() { &[0; 16] } else { bytemuck::cast_slice(of) };
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some(label), contents, usage })
}

/// A scene on a device.
pub struct Room {
    accel: Accel,
    corners: wgpu::Buffer,
    faces: wgpu::Buffer,
    data: wgpu::Buffer,
    texels: wgpu::Buffer,
    opaque: u32,
    at: [u32; 4],
    rays: u32,
    triangles: u32,
}

impl Room {
    /// The buffers are uploaded with the next submit; the structures have to be built ([`Room::record_build`]) before
    /// anything is encoded with them.
    pub fn new(device: &wgpu::Device, tables: &Tables) -> Result<Room, String> {
        let accel = Accel::new(device, tables)?;
        let storage = wgpu::BufferUsages::STORAGE;
        scoped(device, "the scene's tables", || Room {
            corners: held(device, "rtgi_live_corners", tables.corners(), storage | wgpu::BufferUsages::BLAS_INPUT),
            faces: held(device, "rtgi_live_faces", tables.faces(), storage),
            data: held(device, "rtgi_live_data", tables.data(), storage),
            texels: held(device, "rtgi_live_texels", tables.texels(), storage),
            opaque: tables.opaque(),
            at: tables.starts(),
            rays: tables.rays(),
            triangles: tables.triangles(),
            accel,
        })
    }

    /// Records the structures' builds. What is wrong with them shows where the encoder is finished and submitted:
    /// within [`scoped`] it comes back as text.
    pub fn record_build(&self, encoder: &mut wgpu::CommandEncoder) {
        self.accel.record(encoder, &self.corners);
    }

    /// [`Room::record_build`] in a submit of its own.
    pub fn build(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> Result<(), String> {
        scoped(device, "building the structures", || {
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("rtgi_live_build") });
            self.record_build(&mut encoder);
            queue.submit([encoder.finish()]);
        })
    }

    /// Of its buffers, without the structures.
    pub fn bytes(&self) -> u64 {
        [&self.corners, &self.faces, &self.data, &self.texels].iter().map(|b| b.size()).sum()
    }
}

/// What is asked of the solves of a room.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Config {
    /// Of a batch; fewer where the rays would be more than [`MOST_RAYS_A_DISPATCH`].
    pub points: u32,
    /// As [`nvf_rtgi::GatherConfig::emission`]: 0..=1.
    pub emission: f32,
    /// Where the structure's queries start: of a ray in units of its direction, of a shadow ray of its segment. The
    /// solver's is an open 0, which the least positive numbers are: a triangle at a computed 0 is not met whether the
    /// driver takes the near end as open or not. Above that it is for looking into differences, and it is the
    /// structure's alone: the solver's own test, which decides a point's listed triangles and those in the place of
    /// the structure's nearest, meets above 0 whatever it is, as on the CPU.
    pub near: f32,
}

impl Default for Config {
    fn default() -> Config {
        Config { points: 512, emission: 0.25, near: 1e-30 }
    }
}

/// How many points a batch has, of `asked` points with `rays` each, where no hit is in the reach of more lamps than
/// `lamps`.
fn batch_size(asked: u32, rays: u32, lamps: u32, most_groups: u32) -> u32 {
    let shadowed = MOST_WITH_SHADOWS / rays.max(1).saturating_mul(lamps.saturating_add(1));
    asked.min(MOST_RAYS_A_DISPATCH / rays.max(1)).min(shadowed).min(most_groups).max(1)
}

/// The points of a room, the light, and where the light around them goes.
pub struct Live {
    kernel: Arc<Kernel>,
    room: Arc<Room>,
    places: wgpu::Buffer,
    params: wgpu::Buffer,
    lights: wgpu::Buffer,
    out: wgpu::Buffer,
    bind: wgpu::BindGroup,
    count: u32,
    batch: u32,
}

impl Live {
    pub fn new(device: &wgpu::Device, kernel: Arc<Kernel>, room: Arc<Room>, points: &Points, light: &LightState, config: &Config) -> Result<Live, String> {
        Live::packed(device, kernel, room, points, &PackedLight::of(light)?, config)
    }

    /// [`Live::new`] with light that was packed elsewhere.
    pub fn packed(device: &wgpu::Device, kernel: Arc<Kernel>, room: Arc<Room>, points: &Points, light: &PackedLight, config: &Config) -> Result<Live, String> {
        let limits = device.limits();
        let Config { emission, near, .. } = *config;
        // A solve's emission (nvf-rtgi reach.rs `EMISSIONS`); the light of a ray is at most 1 then.
        if !((0.0..=1.0).contains(&emission) && near > 0.0 && near < 1.0) {
            return Err(format!("{config:?}"));
        }
        let count = u32::try_from(points.positions().len()).map_err(|_| "too many points")?;
        let places = points.words()?;
        let order_at = PLACE as u32 * count;
        let lights = light.words();
        let batch = batch_size(config.points, room.rays, light.most_lamps(), limits.max_compute_workgroups_per_dimension);
        let batches = count.div_ceil(batch);
        let out_bytes = match kernel.target {
            Target::Moments => u64::from(count) * MOMENTS,
            Target::Reach => u64::from(count) * REACH,
            Target::Replay => u64::from(count) * u64::from(room.rays) * ANSWER,
            Target::Meetings if count > room.triangles => return Err(format!("{count} points for {} triangles", room.triangles)),
            Target::Meetings => u64::from(count) * MEETING,
        };
        let most = limits.max_buffer_size.min(limits.max_storage_buffer_binding_size);
        for (what, bytes) in [("points", 4 * places.len() as u64), ("what is written", out_bytes), ("lights", 4 * lights.len() as u64), ("batches", SLOT * u64::from(batches))] {
            if bytes > most {
                return Err(format!("oversized: {bytes} bytes of {what}, the device allows {most}"));
            }
        }
        let mut params = vec![0u32; batches as usize * SLOT as usize / 4];
        let [textures, colors, linear, directions] = room.at;
        for (i, slot) in params.chunks_mut(SLOT as usize / 4).enumerate() {
            let first = i as u32 * batch;
            let words = [first, batch.min(count - first), room.rays, room.opaque, textures, colors, linear, directions, order_at, emission.to_bits(), near.to_bits()];
            slot[..words.len()].copy_from_slice(&words);
        }
        let usage = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST;
        let (places, params, lights, out, bind) = scoped(device, "a room's points", || {
            let places = held(device, "rtgi_live_points", &places, usage);
            let params = held(device, "rtgi_live_params", &params, wgpu::BufferUsages::UNIFORM);
            let lights = held(device, "rtgi_live_lights", lights, usage);
            let out = device.create_buffer(&wgpu::BufferDescriptor { label: Some("rtgi_live_out"), size: out_bytes, usage: usage | wgpu::BufferUsages::COPY_SRC, mapped_at_creation: false });
            let bind = bound(device, &kernel, &room, [&places, &params, &lights, &out]);
            (places, params, lights, out, bind)
        })?;
        Ok(Live { kernel, room, places, params, lights, out, bind, count, batch })
    }

    pub fn points(&self) -> u32 {
        self.count
    }

    /// Batches of a whole round, and the points of each but the last.
    pub fn batches(&self) -> u32 {
        self.count.div_ceil(self.batch)
    }

    pub fn batch_points(&self) -> u32 {
        self.batch
    }

    /// One batch, in a pass of its own: the points at its place in the order, each whole. The structures have to be
    /// built in this encoder or before.
    pub fn encode(&self, encoder: &mut wgpu::CommandEncoder, batch: u32, timestamps: Option<wgpu::ComputePassTimestampWrites<'_>>) {
        if batch >= self.batches() {
            return;
        }
        let count = self.batch.min(self.count - batch * self.batch);
        // A point a workgroup, or a ray an invocation.
        let groups = match self.kernel.target {
            Target::Moments | Target::Reach => count,
            Target::Replay => (count * self.room.rays).div_ceil(64),
            Target::Meetings => count.div_ceil(64),
        };
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("rtgi_live"), timestamp_writes: timestamps });
        pass.set_pipeline(&self.kernel.pipeline);
        pass.set_bind_group(0, &self.bind, &[batch * SLOT as u32]);
        pass.dispatch_workgroups(groups, 1, 1);
    }

    /// What the batches write, by the points' numbers (of answers by their order's): for [`probes_of`], [`reach_of`],
    /// [`answers_of`] or [`meetings_of`], by the kernel's target.
    pub fn out(&self) -> &wgpu::Buffer {
        &self.out
    }

    /// What is held beside the room, by its name.
    pub fn bytes(&self) -> Vec<(&'static str, u64)> {
        vec![("points", self.places.size()), ("lights", self.lights.size()), ("batches", self.params.size()), ("what is written", self.out.size())]
    }
}

fn bound(device: &wgpu::Device, kernel: &Kernel, room: &Room, [places, params, lights, out]: [&wgpu::Buffer; 4]) -> wgpu::BindGroup {
    let buffers = [(1, &room.corners), (2, &room.faces), (3, &room.data), (4, &room.texels), (5, lights), (6, places), (8, out)];
    let mut entries: Vec<_> = buffers.into_iter().map(|(binding, b)| wgpu::BindGroupEntry { binding, resource: b.as_entire_binding() }).collect();
    entries.push(wgpu::BindGroupEntry { binding: 0, resource: room.accel.tlas.as_binding() });
    let slot = wgpu::BufferBinding { buffer: params, offset: 0, size: wgpu::BufferSize::new(4 * PARAMS as u64) };
    entries.push(wgpu::BindGroupEntry { binding: 7, resource: wgpu::BindingResource::Buffer(slot) });
    device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("rtgi_live"), layout: &kernel.layout, entries: &entries })
}

/// The bytes of a buffer once the GPU is done with what was submitted: copied, mapped and waited for.
pub fn read_back(device: &wgpu::Device, queue: &wgpu::Queue, buffer: &wgpu::Buffer) -> Result<Vec<u8>, String> {
    let staging = scoped(device, "a readback", || {
        let staging = device.create_buffer(&wgpu::BufferDescriptor { label: Some("rtgi_live_readback"), size: buffer.size(), usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("rtgi_live_readback") });
        encoder.copy_buffer_to_buffer(buffer, 0, &staging, 0, buffer.size());
        queue.submit([encoder.finish()]);
        staging
    })?;
    let done = Arc::new(Mutex::new(None));
    let set = Arc::clone(&done);
    staging.map_async(wgpu::MapMode::Read, .., move |r| {
        if let Ok(mut d) = set.lock() {
            *d = Some(r.is_ok());
        }
    });
    device.poll(wgpu::PollType::wait_indefinitely()).map_err(|e| format!("waiting for the GPU: {e}"))?;
    if done.lock().map_or(None, |d| *d) != Some(true) {
        return Err("a readback was not mapped".into());
    }
    let bytes = staging.get_mapped_range(..).map_err(|e| format!("a readback: {e}"))?.to_vec();
    staging.unmap();
    Ok(bytes)
}

/// Little-endian words, as a readback has them, `N` a row; what is left over is no row.
fn rows<const N: usize>(bytes: &[u8]) -> Vec<[u32; N]> {
    let words: Vec<u32> = bytes.as_chunks::<4>().0.iter().map(|b| u32::from_le_bytes(*b)).collect();
    words.as_chunks::<N>().0.to_vec()
}

/// The points' [`Probe`]s, of [`Target::Moments`].
pub fn probes_of(bytes: &[u8]) -> Vec<Probe> {
    let of = |w: &[u32; 16]| {
        let f = w.map(f32::from_bits);
        let c = std::array::from_fn(|k| V3([f[4 * k], f[4 * k + 1], f[4 * k + 2]]));
        let n = |k: usize| f[4 * k + 3] as u32;
        Probe { moments: Moments { c }, hits: n(0), backs: n(1), lamps: n(2), shadow_rays: n(3) }
    };
    rows(bytes).iter().map(of).collect()
}

/// Of each point, its hits and of them the backs, of [`Target::Reach`]: valid where hits > 0 and 4 backs <= hits.
pub fn reach_of(bytes: &[u8]) -> Vec<[u32; 2]> {
    rows::<2>(bytes)
}

/// What a ray met, of [`Target::Replay`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Answer {
    pub hit: bool,
    pub hidden: bool,
    pub front: bool,
    /// The solver's test met nothing near what the structure met: this is the structure's.
    pub plain: bool,
    /// The primitive: [`Tables::order`] has the scene's number.
    pub primitive: u32,
    pub t: f32,
    pub weights: [f32; 3],
    pub p: V3,
    pub start: V3,
    pub light: V3,
    /// Of the lamps' grid; `None` outside it.
    pub cell: Option<u32>,
    pub lamps: u32,
    pub shadow_rays: u32,
    /// The lamps in sight.
    pub free: u32,
}

/// Those of all rays, by the points' order and then the directions'; `None` of one that was not written.
pub fn answers_of(bytes: &[u8]) -> Vec<Option<Answer>> {
    let of = |w: &[u32; 20]| {
        let f = |k: usize| f32::from_bits(w[k]);
        let v = |k: usize| V3([f(k), f(k + 1), f(k + 2)]);
        let flag = |bit: u32| w[0] & bit != 0;
        (w[19] == 1).then(|| Answer {
            hit: flag(1),
            hidden: flag(2),
            front: flag(4),
            plain: flag(16),
            primitive: w[1],
            t: f(2),
            weights: v(3).0,
            p: v(6),
            start: v(9),
            light: v(12),
            cell: (w[15] != u32::MAX).then_some(w[15]),
            lamps: w[16],
            shadow_rays: w[17],
            free: w[18],
        })
    };
    rows(bytes).iter().map(of).collect()
}

/// The solver's triangle test, of [`Target::Meetings`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Meeting {
    /// The distance and the weights.
    pub met: Option<(f32, [f32; 3])>,
    /// The edges' functions.
    pub edges: [f32; 3],
}

pub fn meetings_of(bytes: &[u8]) -> Vec<Meeting> {
    let of = |w: &[u32; 8]| {
        let f = w.map(f32::from_bits);
        Meeting { met: (w[0] == 1).then_some((f[1], [f[5], f[6], f[7]])), edges: [f[2], f[3], f[4]] }
    };
    rows(bytes).iter().map(of).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// With the ray query capability, to SPIR-V too; the uniform and the rows are what the Rust side writes.
    #[test]
    fn the_kernel_parses_validates_and_becomes_spirv() {
        let module = naga::front::wgsl::parse_str(WGSL).unwrap_or_else(|e| panic!("{}", e.emit_to_string(WGSL)));
        let mut validator = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::RAY_QUERY);
        let info = validator.validate(&module).unwrap_or_else(|e| panic!("{}", e.emit_to_string(WGSL)));
        let entries: Vec<_> = module.entry_points.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(entries, [Target::Moments, Target::Replay, Target::Reach, Target::Meetings].map(Target::entry));
        let options = naga::back::spv::Options { lang_version: (1, 4), ..Default::default() };
        assert!(!naga::back::spv::write_vec(&module, &info, &options, None).unwrap().is_empty());
        let (_, ty) = module.types.iter().find(|(_, t)| t.name.as_deref() == Some("Params")).unwrap();
        let naga::TypeInner::Struct { members, span } = &ty.inner else { panic!() };
        assert!(*span as usize == 4 * PARAMS && members.iter().enumerate().all(|(i, m)| m.offset as usize == 4 * i));
        assert_eq!([3, 7, 8, 9, 10, 11].map(|i| members[i].name.as_deref().unwrap()), ["opaque", "directions_at", "order_at", "emission", "near", "zero"]);
        // The rows as tables.rs packs them and as the readbacks take them.
        use crate::tables::{ALONG, AXES, DIRECTION, GIVEN, INVERSE, SHEAR};
        let lines = [
            format!("const DIRECTION: u32 = {DIRECTION}u;"),
            format!("const GIVEN: u32 = {GIVEN}u;"),
            format!("const ALONG: u32 = {ALONG}u;"),
            format!("const SHEAR: u32 = {SHEAR}u;"),
            format!("const AXES: u32 = {AXES}u;"),
            format!("const INVERSE: u32 = {INVERSE}u;"),
            format!("const PLACE: u32 = {PLACE}u;"),
            format!("const MOMENTS: u32 = {}u;", MOMENTS / 4),
            format!("const ANSWER: u32 = {}u;", ANSWER / 4),
            format!("const MEETING: u32 = {}u;", MEETING / 4),
            "const FACE: u32 = 21u;".into(),
            "const NEAR: u32 = 7u;".into(),
        ];
        for line in lines {
            assert!(WGSL.contains(&line), "{line}");
        }
    }

    #[test]
    fn batches() {
        assert_eq!(batch_size(512, 512, 24, 65_535), 512);
        assert_eq!([(4096, 512), (4096, 4096), (1, 64), (0, 64), (1 << 20, 1)].map(|(points, rays)| batch_size(points, rays, 0, 65_535)), [512, 64, 1, 1, 65_535]);
        assert_eq!([63, 127, 4096, u32::MAX - 1, u32::MAX].map(|lamps| batch_size(512, 512, lamps, 65_535)), [512, 256, 7, 1, 1]);
    }

    #[test]
    fn readbacks() {
        let mut words = [0u32; 32];
        words[16..].copy_from_slice(&std::array::from_fn::<f32, 16, _>(|i| i as f32).map(f32::to_bits));
        let p = probes_of(bytemuck::cast_slice(&words));
        assert_eq!((p.len(), p[0]), (2, Probe::default()));
        assert_eq!((p[1].hits, p[1].backs, p[1].lamps, p[1].shadow_rays, p[1].moments.c[2]), (3, 7, 11, 15, V3::new(8.0, 9.0, 10.0)));
        assert_eq!(reach_of(bytemuck::cast_slice(&[5u32, 1, 512, 0, 7])), [[5, 1], [512, 0]], "the last word is no row");
        let mut words = [0u32; 40];
        (words[20], words[21], words[35], words[39]) = (1 | 4, 7, u32::MAX, 1);
        let a = answers_of(bytemuck::cast_slice(&words));
        let one = a[1].unwrap();
        assert!(a[0].is_none() && one.hit && one.front && !one.hidden && one.primitive == 7 && one.cell.is_none());
        let mut words = [0u32; 8];
        (words[0], words[1], words[6]) = (1, 2.5f32.to_bits(), 0.25f32.to_bits());
        assert_eq!(meetings_of(bytemuck::cast_slice(&words)), [Meeting { met: Some((2.5, [0.0, 0.25, 0.0])), edges: [0.0; 3] }]);
    }
}
