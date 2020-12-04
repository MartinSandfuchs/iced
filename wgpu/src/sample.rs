use crate::Transformation;
use iced_graphics::layer::Sample;
use iced_native::{video, Rectangle};
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::sync::mpsc;
use wgpu::util::DeviceExt;
use zerocopy::AsBytes;

enum Message {
    CopySample(gstreamer::Sample, wgpu::Buffer),
    Exit,
}

#[derive(Debug)]
struct Stream {
    sender: mpsc::Sender<Message>,
    receiver: mpsc::Receiver<wgpu::Buffer>,
    t_frame: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    jh: Option<std::thread::JoinHandle<()>>,
    // The sample which was most recently processed
    cur_sample: Option<video::Sample>,
    width: u32,
    height: u32,
}

impl Stream {
    fn new(
        device: &wgpu::Device,
        width: u32,
        height: u32,
        bind_group_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        // Use a seperate thread to fill buffers (since this is slow and would block the render
        // thread)
        let (sample_sender, sample_receiver) = mpsc::channel();
        let (buffer_sender, buffer_receiver) = mpsc::channel();
        let jh = std::thread::spawn(move || {
            for msg in sample_receiver.iter() {
                match msg {
                    Message::CopySample(sample, buffer) => {
                        let extract_sample = || {
                            let buffer = sample.get_buffer()?;
                            let map = buffer.map_readable().ok()?;
                            Some(map)
                        };
                        if let Some(map) = extract_sample() {
                            let mut write_mapping =
                                buffer.slice(..).get_mapped_range_mut();
                            write_mapping.copy_from_slice(map.as_slice());
                            drop(write_mapping);
                            buffer.unmap();
                            let _ = buffer_sender.send(buffer);
                        }
                    }
                    Message::Exit => {
                        return;
                    }
                }
            }
        });

        // Create textures
        let texture_extent = wgpu::Extent3d {
            width,
            height,
            depth: 1,
        };
        let t_frame = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: texture_extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Bgra8UnormSrgb,
            usage: wgpu::TextureUsage::SAMPLED | wgpu::TextureUsage::COPY_DST,
        });
        let t_view_frame = t_frame.create_view(&Default::default());

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &bind_group_layout,
            entries: &[
                // Video frame
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&t_view_frame),
                },
            ],
            label: None,
        });

        Self {
            sender: sample_sender,
            receiver: buffer_receiver,
            t_frame,
            bind_group,
            jh: Some(jh),
            cur_sample: None,
            width,
            height,
        }
    }
}

impl std::ops::Drop for Stream {
    fn drop(&mut self) {
        // Properly exit the thread because otherwise wgpu will complain
        self.sender.send(Message::Exit).unwrap();
        if let Some(jh) = self.jh.take() {
            jh.join().unwrap();
        }
    }
}

#[derive(Debug)]
pub struct Pipeline {
    // This bind group contains data shared by all streams
    bind_group: wgpu::BindGroup,
    // Layout for the stream specific bind group
    frame_bind_group_layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,

    bounds: wgpu::Buffer,
    transform: wgpu::Buffer,

    streams: HashMap<u64, Stream>,
}

impl Pipeline {
    pub fn new(device: &wgpu::Device) -> Self {
        let vs_module = device.create_shader_module(wgpu::include_spirv!(
            "shader/sample.vert.spv"
        ));

        let fs_module = device.create_shader_module(wgpu::include_spirv!(
            "shader/sample.frag.spv"
        ));

        // Create the texture sampler
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            lod_min_clamp: -100.0,
            lod_max_clamp: 100.0,
            ..Default::default() // compare: wgpu::CompareFunction::Always,
        });

        let bounds: [f32; 16] = Transformation::identity().into();
        let bounds_buffer =
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bounds.as_bytes(),
                usage: wgpu::BufferUsage::UNIFORM | wgpu::BufferUsage::COPY_DST,
            });
        let transform: [f32; 16] = Transformation::identity().into();
        let transform_buffer =
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: transform.as_bytes(),
                usage: wgpu::BufferUsage::UNIFORM | wgpu::BufferUsage::COPY_DST,
            });

        // Create the bind groups
        let frame_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                entries: &[
                    // Video frame
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStage::FRAGMENT,
                        ty: wgpu::BindingType::SampledTexture {
                            multisampled: false,
                            component_type: wgpu::TextureComponentType::Float,
                            dimension: wgpu::TextureViewDimension::D2,
                        },
                        count: None,
                    },
                ],
                label: None,
            });
        let bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                entries: &[
                    // Bounds matrix
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStage::VERTEX,
                        ty: wgpu::BindingType::UniformBuffer {
                            dynamic: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    // Transformation matrix
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStage::VERTEX,
                        ty: wgpu::BindingType::UniformBuffer {
                            dynamic: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    // Sampler
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStage::FRAGMENT,
                        ty: wgpu::BindingType::Sampler { comparison: false },
                        count: None,
                    },
                ],
                label: None,
            });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &bind_group_layout,
            entries: &[
                // Bounds matrix
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(
                        bounds_buffer.slice(..),
                    ),
                    // resource: wgpu::BindingResource::Buffer {
                    //     buffer: &bounds_buffer,
                    //     range: 0..std::mem::size_of::<[f32; 16]>() as u64,
                    // },
                },
                // Transformation matrix
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Buffer(
                        transform_buffer.slice(..),
                    ),
                    // resource: wgpu::BindingResource::Buffer {
                    //     buffer: &transform_buffer,
                    //     range: 0..std::mem::size_of::<[f32; 16]>() as u64,
                    // },
                },
                // Sampler
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
            label: None,
        });

        // Build the render pipeline
        let pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                bind_group_layouts: &[
                    &bind_group_layout,
                    &frame_bind_group_layout,
                ],
                push_constant_ranges: &[],
            });
        let pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: None,
                layout: Some(&pipeline_layout),
                vertex_stage: wgpu::ProgrammableStageDescriptor {
                    module: &vs_module,
                    entry_point: "main",
                },
                fragment_stage: Some(wgpu::ProgrammableStageDescriptor {
                    module: &fs_module,
                    entry_point: "main",
                }),
                rasterization_state: Some(wgpu::RasterizationStateDescriptor {
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: wgpu::CullMode::None,
                    depth_bias: 0,
                    depth_bias_slope_scale: 0.0,
                    depth_bias_clamp: 0.0,
                    clamp_depth: false,
                }),
                primitive_topology: wgpu::PrimitiveTopology::TriangleList,
                color_states: &[wgpu::ColorStateDescriptor {
                    format: wgpu::TextureFormat::Bgra8UnormSrgb,
                    color_blend: wgpu::BlendDescriptor::REPLACE,
                    alpha_blend: wgpu::BlendDescriptor::REPLACE,
                    write_mask: wgpu::ColorWrite::ALL,
                }],
                depth_stencil_state: None,
                vertex_state: wgpu::VertexStateDescriptor {
                    index_format: wgpu::IndexFormat::Uint16,
                    vertex_buffers: &[],
                },
                sample_count: 1,
                sample_mask: !0,
                alpha_to_coverage_enabled: false,
            });

        Self {
            bind_group,
            frame_bind_group_layout,
            pipeline,
            bounds: bounds_buffer,
            transform: transform_buffer,
            streams: HashMap::new(),
        }
    }

    pub fn draw(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        samples: &[Sample],
        transformation: Transformation,
        bounds: Rectangle<u32>,
        target: &wgpu::TextureView,
    ) {
        // Set the transformation matrix
        let mat: [f32; 16] = transformation.into();
        let transform_buffer =
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: mat.as_bytes(),
                usage: wgpu::BufferUsage::COPY_SRC,
            });
        encoder.copy_buffer_to_buffer(
            &transform_buffer,
            0,
            &self.transform,
            0,
            std::mem::size_of::<[f32; 16]>() as u64,
        );
        let layout = &self.frame_bind_group_layout;

        for sample in samples {
            let Sample {
                sample,
                bounds: sample_bounds,
            } = sample;
            let (width, height) = (sample.width as u32, sample.height as u32);

            let entry = self.streams.entry(sample.stream_id);

            // If we see this stream for the first time or if its resolution has changed, we need to
            // create a new stream with the sample's resolution
            let stream = match entry {
                Entry::Occupied(oe) => {
                    let stream = oe.into_mut();
                    if (stream.width, stream.height) != (width, height) {
                        *stream = Stream::new(
                            device,
                            width,
                            height,
                            &self.frame_bind_group_layout,
                        );
                    }
                    stream
                }
                Entry::Vacant(ve) => {
                    ve.insert(Stream::new(device, width, height, layout))
                }
            };

            // Send the sample to the background thread if we haven't already
            if Some(sample) != stream.cur_sample.as_ref() {
                // We could possibly try to reuse the buffers, not sure if this
                // makes a big difference
                let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                    label: None,
                    size: (stream.width * stream.height * 4) as u64,
                    usage: wgpu::BufferUsage::MAP_WRITE
                        | wgpu::BufferUsage::COPY_SRC,
                    mapped_at_creation: false,
                });
                let fut = buffer.slice(..).map_async(wgpu::MapMode::Write);
                device.poll(wgpu::Maintain::Wait);
                futures::executor::block_on(fut).unwrap();

                let _ = stream.sender.send(Message::CopySample(
                    sample.gst_sample.clone(),
                    buffer,
                ));
            }

            // Check if new buffers are available
            let mut last_buffer = stream.receiver.try_iter().last();

            // Draw prerolls immediately (this is required in order to display the correct frame
            if sample.from_preroll {
                // Only upload the preroll if we did not already
                if Some(sample) != stream.cur_sample.as_ref() {
                    let gst_buffer = sample.gst_sample.get_buffer().unwrap();
                    let map = gst_buffer.map_readable().ok().unwrap();
                    let buffer = device.create_buffer_init(
                        &wgpu::util::BufferInitDescriptor {
                            label: None,
                            contents: map.as_slice(),
                            usage: wgpu::BufferUsage::COPY_SRC,
                        },
                    );
                    last_buffer = Some(buffer)
                }
            }

            if let Some(buffer) = last_buffer {
                // Upload the sample
                let texture_extent = wgpu::Extent3d {
                    width: stream.width,
                    height: stream.height,
                    depth: 1,
                };
                encoder.copy_buffer_to_texture(
                    wgpu::BufferCopyView {
                        buffer: &buffer,
                        layout: wgpu::TextureDataLayout {
                            offset: 0,
                            bytes_per_row: 4 * stream.width,
                            rows_per_image: stream.height,
                        },
                    },
                    wgpu::TextureCopyView {
                        texture: &stream.t_frame,
                        mip_level: 0,
                        origin: wgpu::Origin3d { x: 0, y: 0, z: 0 },
                    },
                    texture_extent,
                );
            }

            // Set the sample's bounds matrix
            #[rustfmt::skip]
            let mat: [f32; 16] = [
                sample_bounds.width, 0.0, 0.0, 0.0,
                0.0, sample_bounds.height, 0.0, 0.0,
                0.0, 0.0, 0.0, 0.0,
                sample_bounds.x, sample_bounds.y, 0.0, 1.0,
            ];
            let bounds_buffer =
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: None,
                    contents: mat.as_bytes(),
                    usage: wgpu::BufferUsage::COPY_SRC,
                });
            encoder.copy_buffer_to_buffer(
                &bounds_buffer,
                0,
                &self.bounds,
                0,
                std::mem::size_of::<[f32; 16]>() as u64,
            );

            let mut render_pass =
                encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    color_attachments: &[
                        wgpu::RenderPassColorAttachmentDescriptor {
                            attachment: target,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Load,
                                store: true,
                            },
                        },
                    ],
                    depth_stencil_attachment: None,
                });
            render_pass.set_pipeline(&self.pipeline);
            render_pass.set_scissor_rect(
                bounds.x,
                bounds.y,
                bounds.width,
                bounds.height,
            );
            render_pass.set_bind_group(0, &self.bind_group, &[]);
            render_pass.set_bind_group(1, &stream.bind_group, &[]);
            render_pass.draw(0..6, 0..1);

            // Set the new frame to the active frame
            stream.cur_sample = Some(sample.clone());
        }
    }
}
