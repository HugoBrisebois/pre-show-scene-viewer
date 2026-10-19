use eframe::egui::epaint::PaintCallback;
use eframe::egui_glow::CallbackFn;
use eframe::glow::{self, HasContext as _};
use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke};
use std::f32::consts::TAU;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, mpsc::SyncSender};
use std::time::{SystemTime, UNIX_EPOCH};

pub const SPECTRUM_BINS: usize = 513;
pub const LOOP_SECONDS: f32 = 16.0;
const VJ_SEED_MASK: u32 = 0x00ff_ffff;
static VJ_SEED_COUNTER: AtomicU32 = AtomicU32::new(0);

pub struct ExportCapture {
    pub width: u32,
    pub height: u32,
    pub frame_count: usize,
    pub next_frame: usize,
    pub vj_loops: bool,
    pub multi_band_vj: bool,
    pub audio_reactive: bool,
    pub vj_seed: f32,
    pub audio_controls: Vec<[f32; 4]>,
    pub primary_color: Color32,
    pub secondary_color: Color32,
    pub frame_sender: SyncSender<crate::video_export::ExportMessage>,
}

#[derive(Default)]
pub struct ExportController {
    pub capture: Option<ExportCapture>,
    pub error: Option<String>,
}

pub fn new_vj_seed() -> f32 {
    let time_seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos()
        & VJ_SEED_MASK;
    let sequence = VJ_SEED_COUNTER.fetch_add(1, Ordering::Relaxed) & VJ_SEED_MASK;
    time_seed.wrapping_add(sequence) as f32 / (VJ_SEED_MASK + 1) as f32
}

pub fn export_audio_controls(spectrum: &[f32], frame_count: usize, fps: u32) -> Vec<[f32; 4]> {
    let delta_seconds = 1.0 / fps.max(1) as f32;
    let mut smoother = AudioSmoother::default();
    let mut controls = Vec::with_capacity(frame_count);
    for frame_index in 0..frame_count {
        let phase = frame_index as f32 / frame_count.max(1) as f32;
        let (bass, mids, treble) = spectrum_energies(spectrum, phase);
        let (bass, mids, treble, onset) = smoother.update(bass, mids, treble, delta_seconds);
        controls.push([bass, mids, treble, onset]);
    }
    controls
}

pub struct VjShaderPipeline {
    gl: Arc<glow::Context>,
    program: glow::Program,
    vertex_array: glow::VertexArray,
    buffer: glow::Buffer,
    time_uniform: Option<glow::UniformLocation>,
    bass_uniform: Option<glow::UniformLocation>,
    mids_uniform: Option<glow::UniformLocation>,
    treble_uniform: Option<glow::UniformLocation>,
    onset_uniform: Option<glow::UniformLocation>,
    aspect_uniform: Option<glow::UniformLocation>,
    primary_color_uniform: Option<glow::UniformLocation>,
    secondary_color_uniform: Option<glow::UniformLocation>,
    vj_loops_uniform: Option<glow::UniformLocation>,
    multi_band_vj_uniform: Option<glow::UniformLocation>,
    audio_reactive_uniform: Option<glow::UniformLocation>,
    vj_seed_uniform: Option<glow::UniformLocation>,
    uniform_warning: Option<String>,
    export_target: Arc<Mutex<Option<OffscreenTarget>>>,
}

struct OffscreenTarget {
    framebuffer: glow::Framebuffer,
    texture: glow::Texture,
    width: u32,
    height: u32,
}

#[derive(Default)]
pub struct AudioSmoother {
    bass: f32,
    mids: f32,
    treble: f32,
    previous_bass: f32,
    onset: f32,
}

impl AudioSmoother {
    fn update(
        &mut self,
        bass: f32,
        mids: f32,
        treble: f32,
        delta_seconds: f32,
    ) -> (f32, f32, f32, f32) {
        let delta_seconds = delta_seconds.clamp(0.0, 0.1);
        let onset_strength = ((bass - self.previous_bass * 1.12 - 0.018) / 0.16).clamp(0.0, 1.0);
        self.onset = self.onset.max(onset_strength) * (-delta_seconds / 0.24).exp();
        self.previous_bass = bass;
        self.bass = smooth_audio_level(self.bass, bass, delta_seconds);
        self.mids = smooth_audio_level(self.mids, mids, delta_seconds);
        self.treble = smooth_audio_level(self.treble, treble, delta_seconds);
        (self.bass, self.mids, self.treble, self.onset)
    }
}

fn smooth_audio_level(current: f32, target: f32, delta_seconds: f32) -> f32 {
    let time_constant = if target > current { 0.045 } else { 0.28 };
    let blend = 1.0 - (-delta_seconds / time_constant).exp();
    current + (target - current) * blend
}

impl VjShaderPipeline {
    pub fn new(gl: Arc<glow::Context>) -> Result<Self, String> {
        let vertex_shader = compile_shader(
            &gl,
            glow::VERTEX_SHADER,
            include_str!("shaders/vertex.glsl"),
        )?;
        let fragment_shader = match compile_shader(
            &gl,
            glow::FRAGMENT_SHADER,
            include_str!("shaders/fragments.glsl"),
        ) {
            Ok(shader) => shader,
            Err(error) => {
                unsafe { gl.delete_shader(vertex_shader) };
                return Err(error);
            }
        };

        let program = unsafe { gl.create_program() }.map_err(|error| {
            unsafe {
                gl.delete_shader(vertex_shader);
                gl.delete_shader(fragment_shader);
            }
            error
        })?;

        unsafe {
            gl.attach_shader(program, vertex_shader);
            gl.attach_shader(program, fragment_shader);
            gl.link_program(program);
            gl.delete_shader(vertex_shader);
            gl.delete_shader(fragment_shader);
        }
        if !unsafe { gl.get_program_link_status(program) } {
            let error = unsafe { gl.get_program_info_log(program) };
            unsafe { gl.delete_program(program) };
            return Err(format!("Could not link reactive shader program: {error}"));
        }

        let uniforms = unsafe {
            (
                gl.get_uniform_location(program, "u_time"),
                gl.get_uniform_location(program, "u_bass"),
                gl.get_uniform_location(program, "u_mids"),
                gl.get_uniform_location(program, "u_treble"),
                gl.get_uniform_location(program, "u_onset"),
                gl.get_uniform_location(program, "u_aspect"),
                gl.get_uniform_location(program, "u_primary_color"),
                gl.get_uniform_location(program, "u_secondary_color"),
                gl.get_uniform_location(program, "u_vj_loops"),
                gl.get_uniform_location(program, "u_multiband_vj"),
                gl.get_uniform_location(program, "u_vj_seed"),
                gl.get_uniform_location(program, "u_audio_reactive"),
            )
        };
        let (
            time_uniform,
            bass_uniform,
            mids_uniform,
            treble_uniform,
            onset_uniform,
            aspect_uniform,
            primary_color_uniform,
            secondary_color_uniform,
            vj_loops_uniform,
            multi_band_vj_uniform,
            vj_seed_uniform,
            audio_reactive_uniform,
        ) = uniforms;
        let missing_uniforms = [
            ("u_time", time_uniform.is_none()),
            ("u_bass", bass_uniform.is_none()),
            ("u_mids", mids_uniform.is_none()),
            ("u_treble", treble_uniform.is_none()),
            ("u_onset", onset_uniform.is_none()),
            ("u_aspect", aspect_uniform.is_none()),
            ("u_primary_color", primary_color_uniform.is_none()),
            ("u_secondary_color", secondary_color_uniform.is_none()),
            ("u_vj_loops", vj_loops_uniform.is_none()),
            ("u_multiband_vj", multi_band_vj_uniform.is_none()),
            ("u_vj_seed", vj_seed_uniform.is_none()),
            ("u_audio_reactive", audio_reactive_uniform.is_none()),
        ]
        .into_iter()
        .filter_map(|(name, missing)| missing.then_some(name))
        .collect::<Vec<_>>();
        let uniform_warning = (!missing_uniforms.is_empty()).then(|| {
            format!(
                "OpenGL optimized out or could not locate shader uniform(s): {}",
                missing_uniforms.join(", ")
            )
        });

        let (vertex_array, buffer) = unsafe { (gl.create_vertex_array(), gl.create_buffer()) };
        let (vertex_array, buffer) = match (vertex_array, buffer) {
            (Ok(vertex_array), Ok(buffer)) => (vertex_array, buffer),
            (vertex_array, buffer) => {
                let error = match (&vertex_array, &buffer) {
                    (Err(error), _) | (_, Err(error)) => error.clone(),
                    _ => "unknown OpenGL allocation error".to_owned(),
                };
                let vertex_array = vertex_array.ok();
                let buffer = buffer.ok();
                unsafe {
                    gl.delete_program(program);
                    if let Some(vertex_array) = vertex_array {
                        gl.delete_vertex_array(vertex_array);
                    }
                    if let Some(buffer) = buffer {
                        gl.delete_buffer(buffer);
                    }
                }
                return Err(format!(
                    "Could not allocate reactive shader geometry: {error}"
                ));
            }
        };

        let vertices: [f32; 12] = [
            -1.0, -1.0, 1.0, -1.0, 1.0, 1.0, -1.0, -1.0, 1.0, 1.0, -1.0, 1.0,
        ];
        unsafe {
            gl.bind_vertex_array(Some(vertex_array));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(buffer));
            gl.buffer_data_u8_slice(
                glow::ARRAY_BUFFER,
                std::slice::from_raw_parts(
                    vertices.as_ptr().cast::<u8>(),
                    std::mem::size_of_val(&vertices),
                ),
                glow::STATIC_DRAW,
            );
            gl.enable_vertex_attrib_array(0);
            gl.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, 2 * 4, 0);
            gl.bind_buffer(glow::ARRAY_BUFFER, None);
            gl.bind_vertex_array(None);
        }

        Ok(Self {
            gl,
            program,
            vertex_array,
            buffer,
            time_uniform,
            bass_uniform,
            mids_uniform,
            treble_uniform,
            onset_uniform,
            aspect_uniform,
            primary_color_uniform,
            secondary_color_uniform,
            vj_loops_uniform,
            multi_band_vj_uniform,
            vj_seed_uniform,
            audio_reactive_uniform,
            uniform_warning,
            export_target: Arc::new(Mutex::new(None)),
        })
    }

    pub fn uniform_warning(&self) -> Option<&str> {
        self.uniform_warning.as_deref()
    }

    fn paint_callback(
        &self,
        rect: Rect,
        time: f32,
        bass: f32,
        mids: f32,
        treble: f32,
        onset: f32,
        vj_loops: bool,
        audio_reactive: bool,
        vj_seed: f32,
        primary_color: Color32,
        secondary_color: Color32,
        export_controller: Arc<Mutex<ExportController>>,
        multi_band_vj: bool,
    ) -> PaintCallback {
        let gl = Arc::clone(&self.gl);
        let program = self.program;
        let vertex_array = self.vertex_array;
        let time_uniform = self.time_uniform;
        let bass_uniform = self.bass_uniform;
        let mids_uniform = self.mids_uniform;
        let treble_uniform = self.treble_uniform;
        let onset_uniform = self.onset_uniform;
        let aspect_uniform = self.aspect_uniform;
        let primary_color_uniform = self.primary_color_uniform;
        let secondary_color_uniform = self.secondary_color_uniform;
        let vj_loops_uniform = self.vj_loops_uniform;
        let multi_band_vj_uniform = self.multi_band_vj_uniform;
        let vj_seed_uniform = self.vj_seed_uniform;
        let audio_reactive_uniform = self.audio_reactive_uniform;
        let aspect = rect.width() / rect.height().max(1.0);
        let primary_color = color_to_rgb(primary_color);
        let secondary_color = color_to_rgb(secondary_color);
        let export_target = Arc::clone(&self.export_target);

        PaintCallback {
            rect,
            callback: Arc::new(CallbackFn::new(move |_info, _painter| {
                draw_shader(
                    &gl,
                    program,
                    vertex_array,
                    &time_uniform,
                    &bass_uniform,
                    &mids_uniform,
                    &treble_uniform,
                    &onset_uniform,
                    &aspect_uniform,
                    &primary_color_uniform,
                    &secondary_color_uniform,
                    &vj_loops_uniform,
                    &multi_band_vj_uniform,
                    &vj_seed_uniform,
                    &audio_reactive_uniform,
                    time,
                    bass,
                    mids,
                    treble,
                    onset,
                    vj_loops,
                    multi_band_vj,
                    audio_reactive,
                    vj_seed,
                    aspect,
                    primary_color,
                    secondary_color,
                );
                render_export_frame(
                    &gl,
                    program,
                    vertex_array,
                    &time_uniform,
                    &bass_uniform,
                    &mids_uniform,
                    &treble_uniform,
                    &onset_uniform,
                    &aspect_uniform,
                    &primary_color_uniform,
                    &secondary_color_uniform,
                    &vj_loops_uniform,
                    &multi_band_vj_uniform,
                    &vj_seed_uniform,
                    &audio_reactive_uniform,
                    &export_target,
                    &export_controller,
                );
            })),
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_shader(
    gl: &glow::Context,
    program: glow::Program,
    vertex_array: glow::VertexArray,
    time_uniform: &Option<glow::UniformLocation>,
    bass_uniform: &Option<glow::UniformLocation>,
    mids_uniform: &Option<glow::UniformLocation>,
    treble_uniform: &Option<glow::UniformLocation>,
    onset_uniform: &Option<glow::UniformLocation>,
    aspect_uniform: &Option<glow::UniformLocation>,
    primary_color_uniform: &Option<glow::UniformLocation>,
    secondary_color_uniform: &Option<glow::UniformLocation>,
    vj_loops_uniform: &Option<glow::UniformLocation>,
    multi_band_vj_uniform: &Option<glow::UniformLocation>,
    vj_seed_uniform: &Option<glow::UniformLocation>,
    audio_reactive_uniform: &Option<glow::UniformLocation>,
    time: f32,
    bass: f32,
    mids: f32,
    treble: f32,
    onset: f32,
    vj_loops: bool,
    multi_band_vj: bool,
    audio_reactive: bool,
    vj_seed: f32,
    aspect: f32,
    primary_color: [f32; 3],
    secondary_color: [f32; 3],
) {
    unsafe {
        gl.use_program(Some(program));
        gl.uniform_1_f32(time_uniform.as_ref(), time);
        if let Some(uniform) = bass_uniform {
            gl.uniform_1_f32(Some(uniform), bass);
        }
        if let Some(uniform) = mids_uniform {
            gl.uniform_1_f32(Some(uniform), mids);
        }
        if let Some(uniform) = treble_uniform {
            gl.uniform_1_f32(Some(uniform), treble);
        }
        if let Some(uniform) = onset_uniform {
            gl.uniform_1_f32(Some(uniform), onset);
        }
        if let Some(uniform) = aspect_uniform {
            gl.uniform_1_f32(Some(uniform), aspect);
        }
        if let Some(uniform) = primary_color_uniform {
            gl.uniform_3_f32(
                Some(uniform),
                primary_color[0],
                primary_color[1],
                primary_color[2],
            );
        }
        if let Some(uniform) = secondary_color_uniform {
            gl.uniform_3_f32(
                Some(uniform),
                secondary_color[0],
                secondary_color[1],
                secondary_color[2],
            );
        }
        gl.uniform_1_i32(vj_loops_uniform.as_ref(), i32::from(vj_loops));
        gl.uniform_1_i32(multi_band_vj_uniform.as_ref(), i32::from(multi_band_vj));
        gl.uniform_1_f32(vj_seed_uniform.as_ref(), vj_seed);
        gl.uniform_1_i32(audio_reactive_uniform.as_ref(), i32::from(audio_reactive));
        gl.bind_vertex_array(Some(vertex_array));
        gl.draw_arrays(glow::TRIANGLES, 0, 6);
    }
}

#[allow(clippy::too_many_arguments)]
fn render_export_frame(
    gl: &glow::Context,
    program: glow::Program,
    vertex_array: glow::VertexArray,
    time_uniform: &Option<glow::UniformLocation>,
    bass_uniform: &Option<glow::UniformLocation>,
    mids_uniform: &Option<glow::UniformLocation>,
    treble_uniform: &Option<glow::UniformLocation>,
    onset_uniform: &Option<glow::UniformLocation>,
    aspect_uniform: &Option<glow::UniformLocation>,
    primary_color_uniform: &Option<glow::UniformLocation>,
    secondary_color_uniform: &Option<glow::UniformLocation>,
    vj_loops_uniform: &Option<glow::UniformLocation>,
    multi_band_vj_uniform: &Option<glow::UniformLocation>,
    vj_seed_uniform: &Option<glow::UniformLocation>,
    audio_reactive_uniform: &Option<glow::UniformLocation>,
    export_target: &Arc<Mutex<Option<OffscreenTarget>>>,
    export_controller: &Arc<Mutex<ExportController>>,
) {
    let Ok(mut controller) = export_controller.lock() else {
        return;
    };
    let Some(mut capture) = controller.capture.take() else {
        return;
    };

    if capture.next_frame >= capture.frame_count {
        let _ = capture
            .frame_sender
            .send(crate::video_export::ExportMessage::Finish);
        return;
    }

    let previous_framebuffer = unsafe { gl.get_parameter_framebuffer(glow::FRAMEBUFFER_BINDING) };
    let mut previous_viewport = [0; 4];
    let previous_pack_alignment = unsafe { gl.get_parameter_i32(glow::PACK_ALIGNMENT) };
    let scissor_was_enabled = unsafe { gl.is_enabled(glow::SCISSOR_TEST) };
    unsafe { gl.get_parameter_i32_slice(glow::VIEWPORT, &mut previous_viewport) };

    let target = match ensure_export_target(gl, export_target, capture.width, capture.height) {
        Ok(target) => target,
        Err(error) => {
            let _ = capture
                .frame_sender
                .send(crate::video_export::ExportMessage::Finish);
            controller.error = Some(error);
            unsafe {
                gl.bind_framebuffer(glow::FRAMEBUFFER, previous_framebuffer);
                gl.viewport(
                    previous_viewport[0],
                    previous_viewport[1],
                    previous_viewport[2],
                    previous_viewport[3],
                );
                if scissor_was_enabled {
                    gl.enable(glow::SCISSOR_TEST);
                }
            }
            return;
        }
    };

    let Some([bass, mids, treble, onset]) = capture.audio_controls.get(capture.next_frame).copied()
    else {
        let _ = capture
            .frame_sender
            .send(crate::video_export::ExportMessage::Finish);
        controller.error = Some("Export audio analysis has too few frame values".to_owned());
        return;
    };
    let time =
        capture.next_frame as f32 / crate::video_export::EXPORT_FPS as f32 / LOOP_SECONDS * TAU;
    let aspect = capture.width as f32 / capture.height as f32;
    let primary_color = color_to_rgb(capture.primary_color);
    let secondary_color = color_to_rgb(capture.secondary_color);
    let width = capture.width as i32;
    let height = capture.height as i32;

    unsafe {
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(target.framebuffer));
        gl.viewport(0, 0, width, height);
        gl.disable(glow::SCISSOR_TEST);
        gl.pixel_store_i32(glow::PACK_ALIGNMENT, 1);
        draw_shader(
            gl,
            program,
            vertex_array,
            time_uniform,
            bass_uniform,
            mids_uniform,
            treble_uniform,
            onset_uniform,
            aspect_uniform,
            primary_color_uniform,
            secondary_color_uniform,
            vj_loops_uniform,
            multi_band_vj_uniform,
            vj_seed_uniform,
            audio_reactive_uniform,
            time,
            bass,
            mids,
            treble,
            onset,
            capture.vj_loops,
            capture.multi_band_vj,
            capture.audio_reactive,
            capture.vj_seed,
            aspect,
            primary_color,
            secondary_color,
        );

        let mut pixels = vec![0; capture.width as usize * capture.height as usize * 4];
        gl.read_pixels(
            0,
            0,
            width,
            height,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            glow::PixelPackData::Slice(Some(&mut pixels)),
        );

        let row_bytes = capture.width as usize * 4;
        let mut top_down_pixels = vec![0; pixels.len()];
        for row in 0..capture.height as usize {
            let source_row = capture.height as usize - row - 1;
            let source_start = source_row * row_bytes;
            let destination_start = row * row_bytes;
            top_down_pixels[destination_start..destination_start + row_bytes]
                .copy_from_slice(&pixels[source_start..source_start + row_bytes]);
        }

        gl.bind_framebuffer(glow::FRAMEBUFFER, previous_framebuffer);
        gl.viewport(
            previous_viewport[0],
            previous_viewport[1],
            previous_viewport[2],
            previous_viewport[3],
        );
        gl.pixel_store_i32(glow::PACK_ALIGNMENT, previous_pack_alignment);
        if scissor_was_enabled {
            gl.enable(glow::SCISSOR_TEST);
        }

        match capture
            .frame_sender
            .send(crate::video_export::ExportMessage::Frame(top_down_pixels))
        {
            Ok(()) => capture.next_frame += 1,
            Err(error) => {
                controller.error =
                    Some(format!("Could not send rendered frame to encoder: {error}"));
                return;
            }
        }

        if capture.next_frame >= capture.frame_count {
            let _ = capture
                .frame_sender
                .send(crate::video_export::ExportMessage::Finish);
        } else {
            controller.capture = Some(capture);
        }
    }
}

fn ensure_export_target(
    gl: &glow::Context,
    export_target: &Arc<Mutex<Option<OffscreenTarget>>>,
    width: u32,
    height: u32,
) -> Result<OffscreenTarget, String> {
    let mut current = export_target
        .lock()
        .map_err(|_| "Export render target lock was poisoned".to_owned())?;
    if let Some(target) = current.as_ref()
        && target.width == width
        && target.height == height
    {
        return Ok(OffscreenTarget {
            framebuffer: target.framebuffer,
            texture: target.texture,
            width,
            height,
        });
    }

    if let Some(target) = current.take() {
        unsafe {
            gl.delete_framebuffer(target.framebuffer);
            gl.delete_texture(target.texture);
        }
    }

    let texture = unsafe { gl.create_texture() }
        .map_err(|error| format!("Could not create export texture: {error}"))?;
    unsafe {
        gl.bind_texture(glow::TEXTURE_2D, Some(texture));
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_MIN_FILTER,
            glow::LINEAR as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_MAG_FILTER,
            glow::LINEAR as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_WRAP_S,
            glow::CLAMP_TO_EDGE as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_WRAP_T,
            glow::CLAMP_TO_EDGE as i32,
        );
        gl.tex_image_2d(
            glow::TEXTURE_2D,
            0,
            glow::RGBA8 as i32,
            width as i32,
            height as i32,
            0,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            glow::PixelUnpackData::Slice(None),
        );
    }

    let framebuffer = match unsafe { gl.create_framebuffer() } {
        Ok(framebuffer) => framebuffer,
        Err(error) => {
            unsafe { gl.delete_texture(texture) };
            return Err(format!("Could not create export framebuffer: {error}"));
        }
    };
    unsafe {
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(framebuffer));
        gl.framebuffer_texture_2d(
            glow::FRAMEBUFFER,
            glow::COLOR_ATTACHMENT0,
            glow::TEXTURE_2D,
            Some(texture),
            0,
        );
        let status = gl.check_framebuffer_status(glow::FRAMEBUFFER);
        if status != glow::FRAMEBUFFER_COMPLETE {
            gl.delete_framebuffer(framebuffer);
            gl.delete_texture(texture);
            return Err(format!(
                "Export framebuffer is incomplete (OpenGL status 0x{status:04x})"
            ));
        }
    }

    let target = OffscreenTarget {
        framebuffer,
        texture,
        width,
        height,
    };
    *current = Some(OffscreenTarget {
        framebuffer,
        texture,
        width,
        height,
    });
    Ok(target)
}

fn color_to_rgb(color: Color32) -> [f32; 3] {
    [
        color.r() as f32 / 255.0,
        color.g() as f32 / 255.0,
        color.b() as f32 / 255.0,
    ]
}

impl Drop for VjShaderPipeline {
    fn drop(&mut self) {
        unsafe {
            self.gl.delete_program(self.program);
            self.gl.delete_vertex_array(self.vertex_array);
            self.gl.delete_buffer(self.buffer);
            if let Ok(target) = self.export_target.lock()
                && let Some(target) = target.as_ref()
            {
                self.gl.delete_framebuffer(target.framebuffer);
                self.gl.delete_texture(target.texture);
            }
        }
    }
}

fn compile_shader(
    gl: &glow::Context,
    shader_type: u32,
    source: &str,
) -> Result<glow::Shader, String> {
    let shader = unsafe { gl.create_shader(shader_type) }
        .map_err(|error| format!("Could not create reactive shader: {error}"))?;
    unsafe {
        gl.shader_source(shader, source);
        gl.compile_shader(shader);
    }
    if !unsafe { gl.get_shader_compile_status(shader) } {
        let error = unsafe { gl.get_shader_info_log(shader) };
        unsafe { gl.delete_shader(shader) };
        return Err(format!("Could not compile reactive shader: {error}"));
    }
    Ok(shader)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Scene {
    Aurora,
    Orbit,
    Kaleidoscope,
    Reactive,
    VjLoops,
    MultiBandVj,
}

impl Scene {
    pub const ALL: [Self; 6] = [
        Self::Aurora,
        Self::Orbit,
        Self::Kaleidoscope,
        Self::Reactive,
        Self::VjLoops,
        Self::MultiBandVj,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Aurora => "Aurora",
            Self::Orbit => "Orbit",
            Self::Kaleidoscope => "Kaleidoscope",
            Self::Reactive => "Reactive",
            Self::VjLoops => "VJ Loops",
            Self::MultiBandVj => "Multi-band VJ",
        }
    }

    pub fn uses_shader(self) -> bool {
        matches!(self, Self::Reactive | Self::VjLoops | Self::MultiBandVj)
    }

    pub fn is_vj_mode(self) -> bool {
        matches!(self, Self::VjLoops | Self::MultiBandVj)
    }
}

pub fn render(
    painter: &Painter,
    rect: Rect,
    spectrum: &[f32],
    elapsed_seconds: f32,
    audio_duration_seconds: f32,
    scene: Scene,
    shader_pipeline: Option<&VjShaderPipeline>,
    audio_smoother: &mut AudioSmoother,
    delta_seconds: f32,
    vj_seed: f32,
    audio_reactive: bool,
    primary_color: Color32,
    secondary_color: Color32,
    export_controller: Arc<Mutex<ExportController>>,
) {
    let phase = if audio_duration_seconds > 0.0 {
        (elapsed_seconds / audio_duration_seconds).clamp(0.0, 0.999_999)
    } else {
        elapsed_seconds.rem_euclid(LOOP_SECONDS) / LOOP_SECONDS
    };
    let (bass, mids, highs) = spectrum_energies(spectrum, phase);
    let energy = (bass * 0.45 + mids * 0.35 + highs * 0.2).clamp(0.0, 1.0);

    match scene {
        Scene::Reactive | Scene::VjLoops | Scene::MultiBandVj => {
            if let Some(pipeline) = shader_pipeline {
                let (smoothed_bass, smoothed_mids, smoothed_treble, onset) =
                    audio_smoother.update(bass, mids, highs, delta_seconds);
                let shader_time = if scene.is_vj_mode() && !audio_reactive {
                    vj_loop_time(elapsed_seconds)
                } else {
                    elapsed_seconds / LOOP_SECONDS * TAU
                };
                painter.add(pipeline.paint_callback(
                    rect,
                    shader_time,
                    smoothed_bass,
                    smoothed_mids,
                    smoothed_treble,
                    onset,
                    scene == Scene::VjLoops,
                    audio_reactive,
                    vj_seed,
                    primary_color,
                    secondary_color,
                    Arc::clone(&export_controller),
                    scene == Scene::MultiBandVj,
                ));
            } else {
                painter.rect_filled(rect, 4.0, Color32::from_rgb(7, 10, 24));
            }
        }
        scene => {
            let mut frame = spectrum_frame(spectrum, phase);
            normalize_spectrum(&mut frame);
            painter.rect_filled(rect, 4.0, Color32::from_rgb(7, 10, 24));
            let glow = Color32::from_rgba_unmultiplied(80, 220, 255, (energy * 110.0) as u8);
            painter.circle_filled(rect.center(), rect.width().min(rect.height()) * 0.34, glow);
            match scene {
                Scene::Aurora => draw_aurora(painter, rect, &frame, phase, bass, mids, highs),
                Scene::Orbit => draw_orbit(painter, rect, &frame, phase, bass, mids, highs),
                Scene::Kaleidoscope => {
                    draw_kaleidoscope(painter, rect, &frame, phase, bass, mids, highs)
                }
                Scene::Reactive | Scene::VjLoops | Scene::MultiBandVj => unreachable!(),
            }
        }
    }
}

fn vj_loop_time(elapsed_seconds: f32) -> f32 {
    elapsed_seconds.rem_euclid(LOOP_SECONDS) / LOOP_SECONDS * TAU
}

fn spectrum_energies(spectrum: &[f32], phase: f32) -> (f32, f32, f32) {
    let mut frame = spectrum_frame(spectrum, phase);
    normalize_spectrum(&mut frame);
    (
        band_energy(&frame, 1..32),
        band_energy(&frame, 32..160),
        band_energy(&frame, 160..SPECTRUM_BINS),
    )
}

fn normalize_spectrum(frame: &mut [f32]) {
    let peak = frame.iter().copied().fold(0.0_f32, f32::max);
    if peak > 0.0 {
        for value in frame {
            *value = (*value / peak).sqrt().clamp(0.0, 1.0);
        }
    }
}

fn spectrum_frame(spectrum: &[f32], phase: f32) -> Vec<f32> {
    let frame_count = spectrum.len() / SPECTRUM_BINS;
    if frame_count == 0 {
        return vec![0.0; SPECTRUM_BINS];
    }

    let position = phase * frame_count as f32;
    let first = position.floor() as usize % frame_count;
    let second = (first + 1) % frame_count;
    let blend = position.fract();
    (0..SPECTRUM_BINS)
        .map(|bin| {
            let a = spectrum[first * SPECTRUM_BINS + bin];
            let b = spectrum[second * SPECTRUM_BINS + bin];
            a + (b - a) * blend
        })
        .collect()
}

fn band_energy(frame: &[f32], bins: std::ops::Range<usize>) -> f32 {
    let values = &frame[bins.start.min(frame.len())..bins.end.min(frame.len())];
    if values.is_empty() {
        0.0
    } else {
        (values.iter().map(|value| value * value).sum::<f32>() / values.len() as f32).sqrt()
    }
}

fn draw_aurora(
    painter: &Painter,
    rect: Rect,
    frame: &[f32],
    phase: f32,
    bass: f32,
    mids: f32,
    highs: f32,
) {
    let width = rect.width();
    let height = rect.height();
    let colors = [
        Color32::from_rgb(34, 232, 197),
        Color32::from_rgb(99, 120, 255),
        Color32::from_rgb(235, 76, 196),
        Color32::from_rgb(255, 176, 90),
    ];

    for (ribbon, color) in colors.into_iter().enumerate() {
        let mut points = Vec::with_capacity(181);
        let ribbon_phase = phase * TAU + ribbon as f32 * 1.63;
        for step in 0..=180 {
            let x = step as f32 / 180.0;
            let bin = (x * (SPECTRUM_BINS - 1) as f32) as usize;
            let spectral = frame[bin];
            let wave = (x * TAU * (1.4 + mids * 1.4) + ribbon_phase).sin();
            let detail = (x * TAU * 4.0 - ribbon_phase * 1.4).cos();
            let displacement =
                wave * (0.06 + bass * 0.1) + detail * (0.025 + highs * 0.05) + spectral * 0.08;
            let y =
                rect.center().y + (ribbon as f32 - 1.5) * height * 0.105 + displacement * height;
            points.push(Pos2::new(rect.left() + x * width, y));
        }
        painter.add(Shape::line(
            points.clone(),
            Stroke::new(12.0 + bass * 22.0, color.gamma_multiply(0.08)),
        ));
        painter.add(Shape::line(
            points,
            Stroke::new(1.6 + highs * 2.5, color.gamma_multiply(0.85)),
        ));
    }
}

fn draw_orbit(
    painter: &Painter,
    rect: Rect,
    frame: &[f32],
    phase: f32,
    bass: f32,
    mids: f32,
    highs: f32,
) {
    let center = rect.center();
    let radius = rect.width().min(rect.height()) * 0.2;
    let rotation = phase * TAU;
    let palette = [
        Color32::from_rgb(50, 232, 207),
        Color32::from_rgb(98, 130, 255),
        Color32::from_rgb(245, 91, 195),
    ];

    for ring in 0..4 {
        let ring_radius = radius * (0.72 + ring as f32 * 0.25) + bass * 13.0;
        let color = palette[ring % palette.len()].gamma_multiply(0.22 + ring as f32 * 0.12);
        painter.circle_stroke(center, ring_radius, Stroke::new(1.0, color));
    }

    let spokes = 144;
    for spoke in 0..spokes {
        let angle = spoke as f32 / spokes as f32 * TAU + rotation;
        let bin = spoke * (SPECTRUM_BINS - 1) / spokes;
        let magnitude = frame[bin];
        let low_radius = radius * (0.8 + (angle * 3.0 + rotation).sin() * 0.06);
        let length = 9.0 + magnitude * (30.0 + bass * 40.0) + mids * 7.0;
        let start = Pos2::new(
            center.x + angle.cos() * low_radius,
            center.y + angle.sin() * low_radius,
        );
        let end = Pos2::new(
            center.x + angle.cos() * (low_radius + length),
            center.y + angle.sin() * (low_radius + length),
        );
        let color = palette[spoke % palette.len()].gamma_multiply(0.35 + magnitude * 0.65);
        painter.line_segment([start, end], Stroke::new(1.0 + highs * 1.4, color));
    }

    for orbit in 0..3 {
        let orbit_phase = rotation * (0.5 + orbit as f32 * 0.23) + orbit as f32 * 2.1;
        let orbit_radius = radius * (0.48 + orbit as f32 * 0.2);
        let dot = Pos2::new(
            center.x + orbit_phase.cos() * orbit_radius,
            center.y + orbit_phase.sin() * orbit_radius,
        );
        painter.circle_filled(dot, 2.5 + bass * 4.0, palette[orbit].gamma_multiply(0.8));
    }
}

fn draw_kaleidoscope(
    painter: &Painter,
    rect: Rect,
    frame: &[f32],
    phase: f32,
    bass: f32,
    mids: f32,
    highs: f32,
) {
    let center = rect.center();
    let radius = rect.width().min(rect.height()) * 0.42;
    let palette = [
        Color32::from_rgb(255, 93, 170),
        Color32::from_rgb(114, 113, 255),
        Color32::from_rgb(50, 231, 209),
        Color32::from_rgb(255, 188, 83),
    ];

    for layer in 0..5 {
        let base_radius = radius * (0.22 + layer as f32 * 0.145);
        let mut points = Vec::with_capacity(241);
        for step in 0..=240 {
            let angle = step as f32 / 240.0 * TAU;
            let spectral_angle = (angle * 6.0 / TAU + phase * 6.0).fract();
            let bin = (spectral_angle * (SPECTRUM_BINS - 1) as f32) as usize;
            let spectral = frame[bin];
            let petals = (angle * 6.0 + phase * TAU).cos();
            let detail = (angle * 12.0 - phase * TAU * 2.0).sin();
            let radial = base_radius
                * (1.0 + petals * (0.07 + mids * 0.08) + detail * (0.025 + highs * 0.04))
                + spectral * (6.0 + bass * 22.0);
            points.push(Pos2::new(
                center.x + angle.cos() * radial,
                center.y + angle.sin() * radial,
            ));
        }
        let color = palette[layer % palette.len()];
        painter.add(Shape::line(
            points,
            Stroke::new(
                1.2 + bass * 1.7,
                color.gamma_multiply(0.4 + layer as f32 * 0.1),
            ),
        ));
    }

    for ray in 0..24 {
        let angle = ray as f32 / 24.0 * TAU + phase * TAU * 0.15;
        let bin = ray * (SPECTRUM_BINS - 1) / 24;
        let length = radius * (0.45 + frame[bin] * (0.3 + bass * 0.35));
        let end = Pos2::new(
            center.x + angle.cos() * length,
            center.y + angle.sin() * length,
        );
        painter.line_segment(
            [center, end],
            Stroke::new(1.0, palette[ray % palette.len()].gamma_multiply(0.22)),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AudioSmoother, LOOP_SECONDS, SPECTRUM_BINS, Scene, new_vj_seed, spectrum_frame,
        vj_loop_time,
    };

    #[test]
    fn each_vj_render_gets_a_new_finite_seed() {
        let first = new_vj_seed();
        let second = new_vj_seed();

        assert!(first.is_finite() && (0.0..1.0).contains(&first));
        assert!(second.is_finite() && (0.0..1.0).contains(&second));
        assert_ne!(first, second);
    }

    #[test]
    fn vj_loops_is_a_distinct_shader_scene() {
        assert_eq!(Scene::VjLoops.label(), "VJ Loops");
        assert!(Scene::VjLoops.uses_shader());
        assert_eq!(Scene::MultiBandVj.label(), "Multi-band VJ");
        assert!(Scene::MultiBandVj.uses_shader());
        assert!(Scene::MultiBandVj.is_vj_mode());
        assert!(Scene::VjLoops.is_vj_mode());
        assert!(Scene::Reactive.uses_shader());
        assert!(!Scene::Aurora.uses_shader());
    }

    #[test]
    fn autonomous_vj_time_wraps_at_the_loop_boundary() {
        assert_eq!(vj_loop_time(0.0), vj_loop_time(LOOP_SECONDS));
        assert_eq!(vj_loop_time(3.5), vj_loop_time(LOOP_SECONDS + 3.5));
    }

    #[test]
    fn spectrum_frame_interpolates_and_wraps_between_frames() {
        let mut spectrum = vec![1.0; SPECTRUM_BINS];
        spectrum.extend(vec![3.0; SPECTRUM_BINS]);

        assert!(
            spectrum_frame(&spectrum, 0.25)
                .iter()
                .all(|value| (*value - 2.0).abs() < f32::EPSILON)
        );
        assert!(
            spectrum_frame(&spectrum, 0.75)
                .iter()
                .all(|value| (*value - 2.0).abs() < f32::EPSILON)
        );
    }

    #[test]
    fn spectrum_frame_is_silent_without_analyzed_audio() {
        assert!(spectrum_frame(&[], 0.5).iter().all(|value| *value == 0.0));
    }

    #[test]
    fn audio_smoothing_attacks_faster_than_it_releases() {
        let mut smoother = AudioSmoother::default();
        let (attack, _, _, onset) = smoother.update(1.0, 0.0, 1.0, 0.1);
        let (release, _, _, _) = smoother.update(0.0, 0.0, 0.0, 0.1);

        assert!(attack > 0.8);
        assert!(onset > 0.0);
        assert!(release > 0.0);
        assert!(release < attack);
    }

    #[test]
    fn export_audio_controls_cover_each_requested_frame() {
        let controls = super::export_audio_controls(&[], 480, 30);
        assert_eq!(controls.len(), 480);
        assert!(controls.iter().all(|frame| frame.iter().all(|v| *v == 0.0)));
    }

    #[test]
    fn export_audio_controls_follow_the_entire_spectrum_timeline() {
        let mut spectrum = vec![0.0; SPECTRUM_BINS * 2];
        spectrum[1..32].fill(1.0);
        spectrum[SPECTRUM_BINS + 160..].fill(1.0);

        let controls = super::export_audio_controls(&spectrum, 60, 30);
        assert_eq!(controls.len(), 60);
        assert!(controls[0][0] > controls[0][2]);
        assert!(controls[30][2] > controls[30][0]);
    }

    #[test]
    fn spectrum_energy_identifies_low_mid_and_high_frequency_tones() {
        let mut spectrum = vec![0.0; SPECTRUM_BINS];
        for (tone_bin, expected_band) in [(8, 0), (80, 1), (300, 2)] {
            spectrum.fill(0.0);
            spectrum[tone_bin] = 1.0;
            let (bass, mids, treble) = super::spectrum_energies(&spectrum, 0.0);
            let energies = [bass, mids, treble];
            let strongest_band = energies
                .iter()
                .enumerate()
                .max_by(|(_, left), (_, right)| left.total_cmp(right))
                .map(|(index, _)| index)
                .expect("spectrum should include frequency bands");

            assert_eq!(strongest_band, expected_band);
            assert!(energies[expected_band] > 0.0);
        }
    }
}
