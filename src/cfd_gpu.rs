//! Headless hardware compute backend. Both CPU and WGSL use the same flux and
//! grid; hardware results are compared explicitly rather than assumed identical.
use crate::{
    cfd::{self, Problem, Solution},
    config::Result,
};
use wgpu::util::DeviceExt;
pub fn solve(p: Problem) -> Result<Solution> {
    solve_with_progress(p, |_, _, _| {})
}
pub fn solve_with_progress(
    p: Problem,
    mut progress: impl FnMut(usize, usize, &str),
) -> Result<Solution> {
    let instance = wgpu::Instance::default();
    let adapter = bevy::tasks::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: false,
        compatible_surface: None,
    }))?;
    let info = adapter.get_info();
    if info.device_type == wgpu::DeviceType::Cpu {
        return Err("CFD requested hardware GPU but only a CPU adapter is available".into());
    }
    let (device, queue) =
        bevy::tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))?;
    device.push_error_scope(wgpu::ErrorFilter::Validation);
    let n = p.nx * p.ny;
    let bytes = n * 16;
    if n.div_ceil(64) > device.limits().max_compute_workgroups_per_dimension as usize {
        return Err("CFD grid exceeds GPU dispatch limit".into());
    }
    if bytes > device.limits().max_storage_buffer_binding_size as usize {
        return Err("CFD grid exceeds GPU storage binding limit".into());
    }
    let floats = |v: Vec<f32>| v.into_iter().flat_map(f32::to_le_bytes).collect::<Vec<_>>();
    let initial = floats((0..n).flat_map(|_| p.far.map(|x| x as f32)).collect());
    let state = |label| {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(label),
            contents: &initial,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        })
    };
    let a = state("Euler state A");
    let b = state("Euler state B");
    let mask = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Slip wall mask"),
        contents: &p
            .solid
            .iter()
            .flat_map(|s| u32::from(*s).to_le_bytes())
            .collect::<Vec<_>>(),
        usage: wgpu::BufferUsages::STORAGE,
    });
    let mut params = Vec::new();
    for v in [p.nx as u32, p.ny as u32, 0, 0] {
        params.extend(v.to_le_bytes());
    }
    params.extend(floats(vec![p.dx as f32, p.dy as f32, p.dt as f32, 0.0]));
    params.extend(floats(p.far.map(|x| x as f32).to_vec()));
    let settings = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Euler grid/CFL"),
        contents: &params,
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Rusanov Euler finite volumes"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../assets/shaders/euler.wgsl").into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("Euler GPU"),
        layout: None,
        module: &shader,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });
    let bind = |old: &wgpu::Buffer, next: &wgpu::Buffer| {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Euler ping-pong"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[old, next, &mask, &settings]
                .iter()
                .enumerate()
                .map(|(i, b)| wgpu::BindGroupEntry {
                    binding: i as u32,
                    resource: b.as_entire_binding(),
                })
                .collect::<Vec<_>>(),
        })
    };
    let groups = [bind(&a, &b), bind(&b, &a)];
    progress(0, p.steps, &info.name);
    for start in (0..p.steps).step_by(100) {
        let mut encoder = device.create_command_encoder(&Default::default());
        for step in start..(start + 100).min(p.steps) {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("Euler timestep"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &groups[step % 2], &[]);
            pass.dispatch_workgroups((n as u32).div_ceil(64), 1, 1);
        }
        let submission = queue.submit([encoder.finish()]);
        device.poll(wgpu::PollType::Wait {
            submission_index: Some(submission),
            timeout: None,
        })?;
        progress((start + 100).min(p.steps), p.steps, &info.name);
    }
    let read = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Euler readback"),
        size: (bytes * 2) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    let (last, previous) = if p.steps.is_multiple_of(2) {
        (&a, &b)
    } else {
        (&b, &a)
    };
    encoder.copy_buffer_to_buffer(last, 0, &read, 0, bytes as u64);
    encoder.copy_buffer_to_buffer(previous, 0, &read, bytes as u64, bytes as u64);
    queue.submit([encoder.finish()]);
    let (tx, rx) = std::sync::mpsc::channel();
    read.slice(..).map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    device.poll(wgpu::PollType::wait_indefinitely())?;
    rx.recv()??;
    if let Some(error) = bevy::tasks::block_on(device.pop_error_scope()) {
        return Err(error.to_string().into());
    }
    let mapped = read.slice(..).get_mapped_range();
    let all: Vec<_> = mapped
        .chunks_exact(16)
        .map(|c| {
            std::array::from_fn(|i| {
                f32::from_le_bytes(c[i * 4..i * 4 + 4].try_into().unwrap()) as f64
            })
        })
        .collect();
    let cells = all[..n].to_vec();
    let residual = cells
        .iter()
        .zip(&all[n..])
        .zip(&p.solid)
        .filter(|(_, s)| !**s)
        .map(|((a, b), _)| (a[0] - b[0]).abs() / p.far[0])
        .fold(0.0, f64::max);
    drop(mapped);
    read.unmap();
    cfd::check(&cells, &p.solid)?;
    Ok(Solution {
        residuals: vec![(p.steps, residual)],
        problem: p,
        cells,
        backend: format!(
            "GPU {} / {:?} WGSL f32 planar Euler",
            info.name, info.backend
        ),
    })
}
