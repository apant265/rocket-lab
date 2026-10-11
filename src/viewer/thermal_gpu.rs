use crate::thermal::{History, Problem};
use wgpu::util::DeviceExt;
fn floats(values: impl IntoIterator<Item = f32>) -> Vec<u8> {
    values.into_iter().flat_map(f32::to_le_bytes).collect()
}
/// Uses the Bevy renderer's actual device and queue, never a software adapter
/// selected independently of the 3D viewport.
pub(super) fn solve_with_progress(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    p: &Problem,
    name: &str,
    mut progress: impl FnMut(usize, usize),
) -> Result<History, String> {
    let count = p.columns.len();
    let frames = p.times.len();
    if count == 0 || count > 1024 || frames == 0 {
        return Err("GPU thermal grid supports 1–1024 columns".into());
    }
    let bytes = count * frames * 8;
    if count * frames * 32 > device.limits().max_storage_buffer_binding_size as usize {
        return Err("Thermal boundary history exceeds this GPU's storage limit".into());
    }
    if bytes as u64 > device.limits().max_storage_buffer_binding_size as u64 {
        return Err("Thermal history exceeds this GPU's storage limit".into());
    }
    device.push_error_scope(wgpu::ErrorFilter::Validation);
    let mut cols = vec![];
    let mut links = vec![];
    for c in &p.columns {
        let offset = links.len() / 2;
        for &(i, g) in &c.links {
            links.extend([i as f32, g as f32]);
        }
        cols.extend([
            c.capacity_j_k as f32,
            c.radial_w_k as f32,
            c.area_m2 as f32,
            c.material.thermal.emissivity as f32,
            if c.exposed { 1.0 } else { 0.0 },
            offset as f32,
            c.links.len() as f32,
            c.stage as f32,
        ]);
    }
    if links.is_empty() {
        links.extend([0.0, 0.0]);
    }
    let forcing = p.boundaries.iter().flatten().flat_map(|b| {
        [
            b.dt as f32,
            b.air_k as f32,
            b.recovery_k as f32,
            b.h_w_m2_k as f32,
            b.active_stage as f32,
            b.source_w as f32,
            0.0,
            0.0,
        ]
    });
    let buffer = |label, values: Vec<u8>| {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(label),
            contents: &values,
            usage: wgpu::BufferUsages::STORAGE,
        })
    };
    let columns = buffer("Thermal material coefficients", floats(cols));
    let connections = buffer("Thermal contact graph", floats(links));
    let boundaries = buffer("Flight aeroheating boundary history", floats(forcing));
    let state = buffer(
        "Wall temperatures",
        floats(vec![p.initial_k as f32; count * 2]),
    );
    let history = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("GPU thermal history"),
        size: bytes as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let read = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Thermal readback"),
        size: bytes as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Material heat diffusion"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../../assets/shaders/thermal.wgsl").into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("Thermal finite volumes"),
        layout: None,
        module: &shader,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });
    let layout = pipeline.get_bind_group_layout(0);
    for start in (0..frames).step_by(64) {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("GPU heat propagation"),
        });
        let mut params = vec![];
        for value in [
            count as u32,
            start as u32,
            (start + 64).min(frames) as u32,
            frames as u32,
        ] {
            params.extend(value.to_le_bytes());
        }
        params.extend(floats([p.initial_k as f32, p.max_step as f32, 0.0, 0.0]));
        let settings = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Thermal chunk"),
            contents: &params,
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let bindings = [
            &columns,
            &connections,
            &boundaries,
            &state,
            &history,
            &settings,
        ];
        let entries = bindings
            .iter()
            .enumerate()
            .map(|(binding, b)| wgpu::BindGroupEntry {
                binding: binding as u32,
                resource: b.as_entire_binding(),
            })
            .collect::<Vec<_>>();
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Thermal column buffers"),
            layout: &layout,
            entries: &entries,
        });
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("Wall / axial diffusion"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups(1, 1, 1);
        drop(pass);
        let submission = queue.submit([encoder.finish()]);
        device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: None,
            })
            .map_err(|e| e.to_string())?;
        progress((start + 64).min(frames), frames);
    }
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_buffer_to_buffer(&history, 0, &read, 0, bytes as u64);
    queue.submit([encoder.finish()]);
    let (tx, rx) = std::sync::mpsc::channel();
    read.slice(..).map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|e| e.to_string())?;
    let error = bevy::tasks::block_on(device.pop_error_scope());
    if let Some(error) = error {
        return Err(error.to_string());
    }
    rx.recv_timeout(std::time::Duration::from_secs(30))
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    let data = read.slice(..).get_mapped_range();
    let values = data
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes(b.try_into().unwrap()) as f64)
        .collect::<Vec<_>>();
    if values.iter().any(|v| !v.is_finite() || *v <= 0.0) {
        return Err("GPU thermal result contains invalid temperatures".into());
    }
    let temperatures = values
        .chunks_exact(count * 2)
        .map(|row| row.chunks_exact(2).map(|v| [v[0], v[1]]).collect())
        .collect();
    drop(data);
    read.unmap();
    Ok(History {
        columns: p.columns.clone(),
        times: p.times.clone(),
        temperatures,
        backend: format!("GPU / {name} / WGSL f32"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires a hardware GPU"]
    fn ultra_multi_lane_grid_matches_cpu_heat_diffusion() {
        let instance = wgpu::Instance::default();
        let adapter =
            bevy::tasks::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: None,
            }))
            .unwrap();
        let info = adapter.get_info();
        assert_ne!(info.device_type, wgpu::DeviceType::Cpu);
        let (device, queue) =
            bevy::tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                .unwrap();
        let mut mission = crate::config::Mission::load("examples/kate-anvil-desert.json").unwrap();
        let top = mission.stages.len() - 1;
        for (index, stage) in mission.stages.iter_mut().enumerate() {
            stage.geometry = Some(crate::config::StageGeometry::generic(stage, index == top));
        }
        let flight = crate::flight::Simulator::new(mission).unwrap().run();
        let detail = crate::analysis_detail::Detail::Ultra;
        let prefix = crate::analysis::prefix(&flight, 4., detail.boundary_step());
        let problem = crate::thermal::prepare_with_resolution(&prefix, detail.thermal());
        assert!(problem.columns.len() > 256 && problem.columns.len() <= 1024);
        let gpu = solve_with_progress(&device, &queue, &problem, &info.name, |_, _| {}).unwrap();
        let cpu = crate::thermal::solve(&problem);
        let error = cpu
            .temperatures
            .iter()
            .flatten()
            .flatten()
            .zip(gpu.temperatures.iter().flatten().flatten())
            .map(|(a, b)| (a - b).abs())
            .fold(0., f64::max);
        println!(
            "{}; {} columns; {} frames; full-history max GPU/CPU error {:.6} K",
            info.name,
            problem.columns.len(),
            problem.times.len(),
            error
        );
        assert!(error < 0.25);
    }
}
