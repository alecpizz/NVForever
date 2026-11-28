//! A device of the check's own, the kernels on it, and a round of batches with what it wrote.

use nvf_rtgi::LightState;
use nvf_rtgi_live::{read_back, scoped, Config, Kernel, Live, Points, Room, Tables, Target};
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

/// wgpu's futures are done when the device is polled.
fn block_on<T>(future: impl Future<Output = T>) -> T {
    let mut future = std::pin::pin!(future);
    let mut context = Context::from_waker(Waker::noop());
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(out) => return out,
            Poll::Pending => std::thread::yield_now(),
        }
    }
}

/// Why there is no device: none that traces rays, or another reason.
pub enum Refused {
    Unsupported(String),
    Failed(String),
}

pub const FIXTURE_TARGETS: [Target; 4] = [Target::Moments, Target::Reach, Target::Replay, Target::Meetings];

pub struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    info: wgpu::AdapterInfo,
    kernels: Vec<(Target, Arc<Kernel>)>,
    /// Errors nothing captured, and a lost device.
    errors: Arc<Mutex<Vec<String>>>,
}

impl Gpu {
    /// The Vulkan adapter whose name holds `name` and that traces rays, a discrete one first.
    pub fn open(name: &str, validate: bool, targets: &[Target]) -> Result<Gpu, Refused> {
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = wgpu::Backends::VULKAN;
        desc.flags = if validate { wgpu::InstanceFlags::DEBUG | wgpu::InstanceFlags::VALIDATION } else { wgpu::InstanceFlags::empty() };
        let instance = wgpu::Instance::new(desc);
        let mut lines = Vec::new();
        let mut able: Vec<wgpu::Adapter> = Vec::new();
        for a in block_on(instance.enumerate_adapters(wgpu::Backends::VULKAN)) {
            let (info, traces) = (a.get_info(), a.features().contains(wgpu::Features::EXPERIMENTAL_RAY_QUERY));
            lines.push(format!("{} ({:?}, {}): {}", info.name, info.device_type, info.driver_info, if traces { "ray queries" } else { "NO ray queries" }));
            if traces && info.name.to_lowercase().contains(&name.to_lowercase()) {
                able.push(a);
            }
        }
        able.sort_by_key(|a| a.get_info().device_type != wgpu::DeviceType::DiscreteGpu);
        let Some(adapter) = able.into_iter().next() else {
            return Err(Refused::Unsupported(format!("unsupported: no Vulkan adapter named {name:?} has ray queries:\n  {}", lines.join("\n  "))));
        };
        let most = adapter.limits();
        let required_limits = wgpu::Limits {
            max_buffer_size: most.max_buffer_size,
            max_storage_buffer_binding_size: most.max_storage_buffer_binding_size,
            max_storage_buffers_per_shader_stage: most.max_storage_buffers_per_shader_stage,
            ..wgpu::Limits::default().using_acceleration_structure_values(most)
        };
        let descriptor = wgpu::DeviceDescriptor {
            label: Some("rtgi-live-check"),
            required_features: wgpu::Features::EXPERIMENTAL_RAY_QUERY,
            required_limits,
            // SAFETY: ray queries are used as documented; the check is a program of its own.
            experimental_features: unsafe { wgpu::ExperimentalFeatures::enabled() },
            ..Default::default()
        };
        let info = adapter.get_info();
        let (device, queue) = block_on(adapter.request_device(&descriptor)).map_err(|e| Refused::Failed(format!("{}: {e}", info.name)))?;
        let errors = Arc::new(Mutex::new(Vec::new()));
        let note = |what: &'static str| {
            let errors = Arc::clone(&errors);
            move |text: String| {
                if let Ok(mut e) = errors.lock() {
                    e.push(format!("{what}: {text}"));
                }
            }
        };
        let (uncaptured, lost) = (note("uncaptured error"), note("device lost"));
        device.on_uncaptured_error(Arc::new(move |e: wgpu::Error| uncaptured(e.to_string())));
        device.set_device_lost_callback(move |why, text| lost(format!("{why:?} {text}")));
        let mut kernels = Vec::new();
        for &target in targets {
            kernels.push((target, Arc::new(Kernel::new(&device, target).map_err(Refused::Failed)?)));
        }
        Ok(Gpu { device, queue, info, kernels, errors })
    }

    pub fn describe(&self) -> String {
        format!("{} ({:?}, {:?}, driver {} {})", self.info.name, self.info.device_type, self.info.backend, self.info.driver, self.info.driver_info)
    }

    /// What nothing else captured.
    pub fn sound(&self) -> Result<(), String> {
        let errors = self.errors.lock().map_err(|_| "the errors' lock")?;
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }

    /// The tables on the device, the structures built.
    pub fn room(&self, tables: &Tables) -> Result<Arc<Room>, String> {
        let room = Room::new(&self.device, tables)?;
        room.build(&self.device, &self.queue)?;
        Ok(Arc::new(room))
    }

    pub fn live(&self, target: Target, room: &Arc<Room>, points: &Points, light: &LightState, config: &Config) -> Result<Live, String> {
        let kernel = self.kernels.iter().find(|k| k.0 == target).ok_or("no such kernel")?.1.clone();
        Live::new(&self.device, kernel, room.clone(), points, light, config)
    }

    pub fn submit(&self, live: &Live) -> Result<(), String> {
        scoped(&self.device, "a round", || {
            let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("rtgi_live_check") });
            (0..live.batches()).for_each(|batch| live.encode(&mut encoder, batch, None));
            self.queue.submit([encoder.finish()]);
        })
    }

    /// All batches of the target's kernel in one submit, and what they wrote.
    pub fn run(&self, target: Target, room: &Arc<Room>, points: &Points, light: &LightState, config: &Config) -> Result<Vec<u8>, String> {
        let live = self.live(target, room, points, light, config)?;
        self.submit(&live)?;
        let bytes = read_back(&self.device, &self.queue, live.out())?;
        self.sound()?;
        Ok(bytes)
    }
}
