//! A cell's bounce light, solved on the GPU with ray queries as nvf-rtgi solves it on the CPU (#3009): the same
//! points, directions, surfaces, lamps and sums. This is NVForever's own solver, home-grown for this engine;
//! nvf-rtgi stays the reference that its results are held against. The device, the queue and the encoder come from
//! outside.
//!
//! ```text
//! let tables = Tables::new(&prepared, &nvf_rtgi::directions(512))?;       // any thread
//! let points = Points::of(&volume, &prepared, &tables)?;                  // any thread
//! let kernel = Arc::new(Kernel::new(&device, Target::Moments)?);          // once a device
//! let room = Arc::new(Room::new(&device, &tables)?);
//! let live = Live::new(&device, kernel, room.clone(), &points, &light, &Config::default())?;
//! room.record_build(&mut encoder);                                        // once
//! live.encode(&mut encoder, batch, None);                                 // batch of 0..live.batches()
//! let probes = probes_of(&read_back(&device, &queue, live.out())?);       // after the submit
//! ```
//!
//! [`Tables`] (the scene), [`PackedLight`] (a state of the light) and [`Points`] (where the light is gathered) are the
//! words the kernel reads, packed on the CPU. What a shader could not compute as the solver does is refused there,
//! with the reason, and stays the CPU solver's. A batch replaces its points whole; nothing is blended over frames.
//! The pipelines never go through Bevy's cache: they are in no list of pipelines that a later start makes ahead.
//! `examples/rtgi-live-check` holds the kernel against the CPU on scenes made in code.

#![forbid(unsafe_code)]

mod accel;
mod live;
mod tables;

pub use accel::{fits, scoped, supported};
pub use live::{answers_of, meetings_of, probes_of, reach_of, read_back, Answer, Config, Kernel, Live, Meeting, Room, Target, MOST_RAYS_A_DISPATCH, WGSL};
pub use tables::{PackedLight, Points, Tables};
