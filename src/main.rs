use std::{collections::HashMap, io::BufWriter, path::Path};

use image::{imageops::FilterType, DynamicImage, RgbImage, RgbaImage};

use process::TARGET;
use texture_packer::{
    exporter::ImageExporter, importer::ImageImporter, texture::Texture, TexturePackerConfig,
};
use wgpu::{util::DeviceExt, BindGroup};

use process::TOTAL_SHAPES;

fn main() {
    pollster::block_on(run())
}

pub(crate) fn lin(c: f32) -> f32 {
    if c > 0.04045 {
        ((c + 0.055) / 1.055).powf(2.4)
    } else {
        c / 12.92
    }
}

async fn run() {
    let width = 512;
    let img = image::open(TARGET).unwrap();
    let aspect_ratio = img.width() as f32 / img.height() as f32;
    let height: u32 = (width as f32 * aspect_ratio) as u32;

    let mut target = img.resize(width, height, FilterType::Triangle);

    // convert this image from srgb to linear

    if let Some(buf) = target.as_mut_rgba8() {
        for p in buf.pixels_mut() {
            let r = lin(p[0] as f32 / 255.0);
            let g = lin(p[1] as f32 / 255.0);
            let b = lin(p[2] as f32 / 255.0);
            p[0] = (r * 255.0) as u8;
            p[1] = (g * 255.0) as u8;
            p[2] = (b * 255.0) as u8;
        }
    } else if let Some(buf) = target.as_mut_rgb8() {
        for p in buf.pixels_mut() {
            let r = lin(p[0] as f32 / 255.0);
            let g = lin(p[1] as f32 / 255.0);
            let b = lin(p[2] as f32 / 255.0);
            p[0] = (r * 255.0) as u8;
            p[1] = (g * 255.0) as u8;
            p[2] = (b * 255.0) as u8;
        }
    }

    // State::new uses async code, so we're going to wait for it to finish
    // The instance is a handle to our GPU
    // BackendBit::PRIMARY => Vulkan + Metal + DX12 + Browser WebGPU
    let instance = wgpu::Instance::new(wgpu::Backends::all());

    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
        })
        .await
        .unwrap();
    let (device, queue) = adapter
        .request_device(&Default::default(), None)
        .await
        .unwrap();

    let texture_desc = wgpu::TextureDescriptor {
        size: wgpu::Extent3d {
            width: target.width(),
            height: target.height(),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING,
        label: None,
    };

    let output_texture = device.create_texture(&texture_desc);
    let output_texture_view = output_texture.create_view(&Default::default());

    let temp_texture = device.create_texture(&wgpu::TextureDescriptor {
        size: wgpu::Extent3d {
            width: target.width(),
            height: target.height(),
            depth_or_array_layers: TOTAL_SHAPES as u32,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING,
        label: None,
    });

    let temp_texture_view = temp_texture.create_view(&wgpu::TextureViewDescriptor {
        // dimension: Some(wgpu::TextureViewDimension::D2Array),
        // base_array_layer: 0,
        ..Default::default()
    });
    let temp_texture =
        texture::Texture::from_texture(&device, temp_texture, temp_texture_view).unwrap();

    let temp_texture_bind_group_layout =
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
            label: Some("temp_texture_bind_group_layout"),
        });

    let output_texture =
        texture::Texture::from_texture(&device, output_texture, output_texture_view).unwrap();

    let output_texture_bind_group_layout =
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
            label: Some("output_texture_bind_group_layout"),
        });

    let output_texture_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        layout: &output_texture_bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&output_texture.view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&output_texture.sampler),
            },
        ],
        label: Some("output_texture_bind_group"),
    });

    let u32_size = std::mem::size_of::<u32>() as u32;

    let output_buffer_size = (u32_size * target.width() * target.height()) as wgpu::BufferAddress;
    let output_buffer_desc = wgpu::BufferDescriptor {
        size: output_buffer_size,
        usage: wgpu::BufferUsages::COPY_DST
        // this tells wpgu that we want to read this buffer from the cpu
        | wgpu::BufferUsages::MAP_READ,
        label: None,
        mapped_at_creation: false,
    };
    let output_buffer = device.create_buffer(&output_buffer_desc);

    let packer = shape::pack_textures();

    let exporter = ImageExporter::export(&packer).unwrap();

    let spritesheet = exporter.as_rgba8().unwrap().clone();

    let sheet_texture =
        texture::Texture::from_bytes(&device, &queue, exporter, "sheet.png").unwrap();

    let sheet_bind_group_layout =
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
            label: Some("sheet_bind_group_layout"),
        });

    let sheet_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        layout: &sheet_bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&sheet_texture.view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&sheet_texture.sampler),
            },
        ],
        label: Some("sheet_bind_group"),
    });

    let target_texture =
        texture::Texture::from_bytes(&device, &queue, target.clone(), TARGET).unwrap();

    let target_bind_group_layout =
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],

            label: Some("target_bind_group_layout"),
        });

    let target_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        layout: &target_bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&target_texture.view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&target_texture.sampler),
            },
        ],
        label: Some("sheet_bind_group"),
    });

    let shader = device.create_shader_module(&wgpu::ShaderModuleDescriptor {
        label: Some("Shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
    });

    let size_uniform = SizeUniform {
        width: target.width(),
        height: target.height(),
    };

    let size_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Size buffer"),
        contents: bytemuck::cast_slice(&[size_uniform]),
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    });

    let size_bind_group_layout =
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
            label: Some("size_bind_group_layout"),
        });

    let size_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        layout: &size_bind_group_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: size_buffer.as_entire_binding(),
        }],
        label: Some("size_bind_group"),
    });

    // let tint_uniform = TintUniform {
    //     tint: [[0.0; 4]; TOTAL_SHAPES],
    // };

    // let tint_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
    //     label: Some("Size buffer"),
    //     contents: bytemuck::cast_slice(&[tint_uniform]),
    //     usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    // });

    // let tint_bind_group_layout =
    //     device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
    //         entries: &[wgpu::BindGroupLayoutEntry {
    //             binding: 0,
    //             visibility: wgpu::ShaderStages::FRAGMENT,
    //             ty: wgpu::BindingType::Buffer {
    //                 ty: wgpu::BufferBindingType::Uniform,
    //                 has_dynamic_offset: false,
    //                 min_binding_size: None,
    //             },
    //             count: None,
    //         }],
    //         label: Some("tint_bind_group_layout"),
    //     });

    // let tint_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
    //     layout: &tint_bind_group_layout,
    //     entries: &[wgpu::BindGroupEntry {
    //         binding: 0,
    //         resource: tint_buffer.as_entire_binding(),
    //     }],
    //     label: Some("tint_bind_group"),
    // });

    let render_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Render Pipeline Layout"),
        bind_group_layouts: &[
            &sheet_bind_group_layout,
            &size_bind_group_layout,
            //&tint_bind_group_layout,
        ],
        push_constant_ranges: &[],
    });

    let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Render Pipeline"),
        layout: Some(&render_pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: "vs_main",
            buffers: &[Vertex::desc()],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: "fs_main",
            targets: &[wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            }],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: Some(wgpu::Face::Back),
            // Setting this to anything other than Fill requires Features::NON_FILL_POLYGON_MODE
            polygon_mode: wgpu::PolygonMode::Fill,
            // Requires Features::DEPTH_CLIP_CONTROL
            unclipped_depth: false,
            // Requires Features::CONSERVATIVE_RASTERIZATION
            conservative: false,
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState {
            count: 1,
            mask: !0,
            alpha_to_coverage_enabled: false,
        },
        // If the pipeline will be used with a multiview render pass, this
        // indicates how many array layers the attachments will have.
        multiview: None,
    });
    // cls on windows btw
    // yes rn at least
    // because its doing two slopes
    // like |/| two triangles yes look in output.png if its updated
    // oh is it pasting slope objects understandable

    let diff_storage_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Diff Storage Buffer"),
        contents: bytemuck::cast_slice(&[DiffBuffer {
            diff: [0; TOTAL_SHAPES],
        }]),
        usage: wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_DST
            | wgpu::BufferUsages::MAP_READ,
    });

    let diff_bind_group_layout =
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: false },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
            label: Some("diff_bind_group_layout"),
        });

    let diff_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        layout: &diff_bind_group_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: diff_storage_buffer.as_entire_binding(),
        }],
        label: Some("diff_bind_group"),
    });

    let compute_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Render Pipeline Layout"),
        bind_group_layouts: &[
            &target_bind_group_layout,
            &temp_texture_bind_group_layout,
            &size_bind_group_layout,
            &diff_bind_group_layout,
        ],
        push_constant_ranges: &[],
    });

    let compute_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("Compute Pipeline"),
        layout: Some(&compute_pipeline_layout),
        module: &shader,
        entry_point: "cmp_main",
    });

    let state = State {
        device,
        queue,
        render_pipeline,
        compute_pipeline,
        size_bind_group,
        sheet_bind_group,
        target_bind_group,

        sheet_size: [packer.width(), packer.height()],
        packer: packer.get_frames().clone(),
        size_uniform,
        // tint_uniform,
        // tint_bind_group,
        // tint_buffer,
        output_texture,
        output_texture_bind_group,
        diff_storage_buffer,
        diff_bind_group,
        temp_texture,
        temp_texture_bind_group_layout,
    };

    let mut encoder = state
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

    {
        let _render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Render Pass"),
            color_attachments: &[wgpu::RenderPassColorAttachment {
                view: &state.output_texture.view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.0,
                        g: 0.0,
                        b: 0.0,
                        a: 1.0,
                    }),

                    store: true,
                },
            }],
            depth_stencil_attachment: None,
        });
    }

    state.queue.submit(Some(encoder.finish()));

    process::process(&state, &target, &spritesheet);

    let mut encoder = state
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

    encoder.copy_texture_to_buffer(
        state.output_texture.texture.as_image_copy(),
        wgpu::ImageCopyBuffer {
            buffer: &output_buffer,
            layout: wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some((u32_size * target.width()).try_into().unwrap()),
                rows_per_image: Some(target.height().try_into().unwrap()),
            },
        },
        texture_desc.size,
    );

    state.queue.submit(Some(encoder.finish()));

    {
        let buffer_slice = output_buffer.slice(..);

        // NOTE: We have to create the mapping THEN device.poll() before await
        // the future. Otherwise the application will freeze.
        let mapping = buffer_slice.map_async(wgpu::MapMode::Read);
        state.device.poll(wgpu::Maintain::Wait);
        mapping.await.unwrap();

        let data = buffer_slice.get_mapped_range();

        use image::{ImageBuffer, Rgba};
        let buffer =
            ImageBuffer::<Rgba<u8>, _>::from_raw(target.width(), target.height(), data).unwrap();
        buffer.save("output.png").unwrap();
    }
    output_buffer.unmap();
}

mod image_diff;

pub struct State {
    device: wgpu::Device,
    queue: wgpu::Queue,
    render_pipeline: wgpu::RenderPipeline,
    compute_pipeline: wgpu::ComputePipeline,
    size_bind_group: wgpu::BindGroup,
    sheet_bind_group: wgpu::BindGroup,
    target_bind_group: wgpu::BindGroup,
    //view: wgpu::TextureView,
    sheet_size: [u32; 2],
    packer: HashMap<u16, texture_packer::Frame<u16>>,

    // tint_uniform: TintUniform,
    // tint_buffer: wgpu::Buffer,
    // tint_bind_group: wgpu::BindGroup,
    size_uniform: SizeUniform,

    output_texture: texture::Texture,
    output_texture_bind_group: wgpu::BindGroup,
    diff_storage_buffer: wgpu::Buffer,
    diff_bind_group: wgpu::BindGroup,

    temp_texture: texture::Texture,
    temp_texture_bind_group_layout: wgpu::BindGroupLayout,
}

mod process;
use process::OPACITY;
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [i32; 2],
    tex_coords: [f32; 2],
    tint: [f32; 4],
}

impl Vertex {
    const ATTRIBS: [wgpu::VertexAttribute; 3] =
        wgpu::vertex_attr_array![0 => Sint32x2, 1 => Float32x2, 2 => Float32x4];

    fn desc<'a>() -> wgpu::VertexBufferLayout<'a> {
        use std::mem;

        wgpu::VertexBufferLayout {
            array_stride: mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBS,
        }
    }
}

// We need this for Rust to store our data correctly for the shaders
#[repr(C)]
// This is so we can store this in a buffer
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct SizeUniform {
    width: u32,
    height: u32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
struct TintUniform {
    tint: [[f32; 4]; TOTAL_SHAPES],
}

unsafe impl bytemuck::Zeroable for TintUniform {}
unsafe impl bytemuck::Pod for TintUniform {}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct DiffBuffer {
    diff: [u32; TOTAL_SHAPES],
}

unsafe impl bytemuck::Zeroable for DiffBuffer {}
unsafe impl bytemuck::Pod for DiffBuffer {}

// square with our texture
// const VERTICES: &[Vertex] = &[
//     // triangle 1
//     Vertex {
//         position: [0, 50],
//         tex_coords: [0.0, 0.0],
//     },
//     Vertex {
//         position: [0, 0],
//         tex_coords: [0.0, 1.0],
//     },
//     Vertex {
//         position: [50, 0],
//         tex_coords: [1.0, 1.0],
//     },
//     // triangle 2
//     Vertex {
//         position: [0, 50],
//         tex_coords: [0.0, 0.0],
//     },
//     Vertex {
//         position: [50, 0],
//         tex_coords: [1.0, 1.0],
//     },
//     Vertex {
//         position: [50, 50],
//         tex_coords: [1.0, 0.0],
//     },
// ];

mod texture;

mod shape;
