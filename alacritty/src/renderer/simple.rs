use {
    crate::{
        gl,
        gl::types::*,
        renderer::{
            clear_atlas, create_program, filewatch, get_shader_info_log, load_glyph,
            rects::RenderRect, Atlas, Error, Glyph, GlyphCache, LoadGlyph, LoaderApi,
            ShaderCreationError, ATLAS_SIZE,
        },
    },
    alacritty_terminal::{
        config::{self, Config, Delta, Font, StartupMode},
        index::{Column, Line},
        term::{
            self,
            cell::{self, Flags, MAX_ZEROWIDTH_CHARS},
            color::Rgb,
            CursorKey, RenderableCell, RenderableCellContent, SizeInfo,
        },
    },
    font::{GlyphKey, RasterizedGlyph},
    log::*,
    std::{mem::size_of, path::PathBuf, ptr},
};

fn create_shader(kind: GLenum, source: &str) -> Result<GLuint, ShaderCreationError> {
    let len: [GLint; 1] = [source.len() as GLint];

    let shader = unsafe {
        let shader = gl::CreateShader(kind);
        gl::ShaderSource(shader, 1, &(source.as_ptr() as *const _), len.as_ptr());
        gl::CompileShader(shader);
        shader
    };

    let mut success: GLint = 0;
    unsafe {
        gl::GetShaderiv(shader, gl::COMPILE_STATUS, &mut success);
    }

    if success == GLint::from(gl::TRUE) {
        Ok(shader)
    } else {
        // Read log.
        let log = get_shader_info_log(shader);

        // Cleanup.
        unsafe {
            gl::DeleteShader(shader);
        }

        Err(ShaderCreationError::Compile(PathBuf::new(), log))
    }
}

#[derive(Debug)]
struct Shader {
    kind: GLuint,
    id: GLuint,

    #[cfg(feature = "live-shader-reload")]
    file: filewatch::File,
}

impl Shader {
    #[cfg(feature = "live-shader-reload")]
    fn from_file(kind: GLuint, file_path: &str) -> Self {
        Self { kind, id: 0, file: filewatch::File::new(std::path::Path::new(file_path)) }
    }

    #[cfg(not(feature = "live-shader-reload"))]
    fn from_source(kind: GLuint, src: &str) -> Result<Self, ShaderCreationError> {
        Ok(Self { kind, id: create_shader(kind, src)? })
    }

    fn id(&self) -> GLuint {
        self.id
    }

    fn valid(&self) -> bool {
        self.id != 0
    }

    #[cfg(feature = "live-shader-reload")]
    fn poll(&mut self) -> Result<bool, ShaderCreationError> {
        Ok(match self.file.read_update() {
            Some(src) => {
                let new_id = create_shader(self.kind, &src)?;
                self.delete();
                self.id = new_id;
                true
            }
            _ => false,
        })
    }

    fn delete(&mut self) {
        if self.id > 0 {
            unsafe {
                gl::DeleteShader(self.id);
            }
        }
    }
}

impl Drop for Shader {
    fn drop(&mut self) {
        self.delete();
    }
}

#[derive(Debug)]
pub struct ShaderProgram {
    /// Program id
    id: GLuint,

    #[cfg(feature = "live-shader-reload")]
    vertex_shader: Shader,

    #[cfg(feature = "live-shader-reload")]
    fragment_shader: Shader,
}

impl ShaderProgram {
    #[cfg(not(feature = "live-shader-reload"))]
    fn from_sources(vertex_src: &str, fragment_src: &str) -> Result<Self, ShaderCreationError> {
        let vertex_shader = create_shader(gl::VERTEX_SHADER, vertex_src)?;
        let fragment_shader = create_shader(gl::FRAGMENT_SHADER, fragment_src)?;
        let program = create_program(vertex_shader, fragment_shader)?;

        unsafe {
            gl::DeleteShader(fragment_shader);
            gl::DeleteShader(vertex_shader);
        }

        Ok(Self { id: program })
    }

    #[cfg(feature = "live-shader-reload")]
    fn from_files(
        vertex_path: &'static str,
        fragment_path: &'static str,
    ) -> Result<Self, ShaderCreationError> {
        Ok(Self {
            id: 0,
            vertex_shader: Shader::from_file(gl::VERTEX_SHADER, vertex_path),
            fragment_shader: Shader::from_file(gl::FRAGMENT_SHADER, fragment_path),
        })
    }

    #[cfg(feature = "live-shader-reload")]
    fn valid(&self) -> bool {
        self.id != 0
    }

    #[cfg(feature = "live-shader-reload")]
    fn poll(&mut self) -> Result<bool, ShaderCreationError> {
        Ok(
            if (self.vertex_shader.poll()? || self.fragment_shader.poll()?)
                && (self.fragment_shader.valid() && self.vertex_shader.valid())
            {
                let program = create_program(self.vertex_shader.id, self.fragment_shader.id)?;

                if self.id > 0 {
                    unsafe {
                        gl::DeleteProgram(self.id);
                    }
                }

                self.id = program;
                true
            } else {
                false
            },
        )
    }
}

impl Drop for ShaderProgram {
    fn drop(&mut self) {
        unsafe {
            gl::DeleteProgram(self.id);
        }
    }
}

/// Draw text using glyph refs
///
/// Uniforms are prefixed with "u", and vertex attributes are prefixed with "a".
#[derive(Debug)]
pub struct ScreenShaderProgram {
    program: ShaderProgram,

    /// vec4(pad.xy, resolution.xy)
    u_screen_dim: GLint,

    /// Cell dimensions (pixels).
    u_cell_dim: GLint,

    u_glyph_ref: GLint,
    u_color_fg: GLint,
    u_color_bg: GLint,

    u_atlas: GLint,
}

static SCREEN_SHADER_F_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../res/screen.f.glsl");
static SCREEN_SHADER_V_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../res/screen.v.glsl");
static SCREEN_SHADER_F: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../res/screen.f.glsl"));
static SCREEN_SHADER_V: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../res/screen.v.glsl"));

impl ScreenShaderProgram {
    #[cfg(feature = "live-shader-reload")]
    pub fn new() -> Result<ScreenShaderProgram, ShaderCreationError> {
        let program = ShaderProgram::from_files(SCREEN_SHADER_V_PATH, SCREEN_SHADER_F_PATH)?;
        let mut this = Self {
            program,
            u_screen_dim: -1,
            u_cell_dim: -1,
            u_glyph_ref: -1,
            u_atlas: -1,
            u_color_fg: -1,
            u_color_bg: -1,
        };
        Ok(this)
    }

    #[cfg(not(feature = "live-shader-reload"))]
    pub fn new() -> Result<ScreenShaderProgram, ShaderCreationError> {
        let program = ShaderProgram::from_sources(SCREEN_SHADER_V, SCREEN_SHADER_F)?;
        let mut this = Self {
            program,
            u_screen_dim: -1,
            u_cell_dim: -1,
            u_glyph_ref: -1,
            u_atlas: -1,
            u_color_fg: -1,
            u_color_bg: -1,
        };
        this.update(true);
        Ok(this)
    }

    fn update(&mut self, validate_uniforms: bool) {
        macro_rules! cptr {
            ($thing:expr) => {
                $thing.as_ptr() as *const _
            };
        }

        macro_rules! assert_uniform_valid {
            ($uniform:expr) => {
                assert!($uniform != gl::INVALID_VALUE as i32);
                assert!($uniform != gl::INVALID_OPERATION as i32);
            };
            ( $( $uniform:expr ),* ) => {
                $( assert_uniform_valid!($uniform); )*
            };
        }

        // get uniform locations
        let (screen_dim, cell_dim, atlas, color_bg, color_fg, glyph_ref) = unsafe {
            (
                gl::GetUniformLocation(self.program.id, cptr!(b"screenDim\0")),
                gl::GetUniformLocation(self.program.id, cptr!(b"cellDim\0")),
                gl::GetUniformLocation(self.program.id, cptr!(b"atlas\0")),
                gl::GetUniformLocation(self.program.id, cptr!(b"color_bg\0")),
                gl::GetUniformLocation(self.program.id, cptr!(b"color_fg\0")),
                gl::GetUniformLocation(self.program.id, cptr!(b"glyphRef\0")),
            )
        };

        if validate_uniforms {
            assert_uniform_valid!(screen_dim, cell_dim, atlas, color_bg, color_fg, glyph_ref);
        }

        self.u_screen_dim = screen_dim;
        self.u_cell_dim = cell_dim;
        self.u_glyph_ref = glyph_ref;
        self.u_atlas = atlas;
        self.u_color_fg = color_fg;
        self.u_color_bg = color_bg;
    }

    #[cfg(feature = "live-shader-reload")]
    fn poll(&mut self) -> Result<bool, ShaderCreationError> {
        if self.program.poll()? {
            self.update(false);
            return Ok(true);
        }

        Ok(false)
    }

    // fn update_projection(&self, width: f32, height: f32, padding_x: f32, padding_y: f32) {
    //     // Bounds check.
    //     if (width as u32) < (2 * padding_x as u32) || (height as u32) < (2 * padding_y as u32) {
    //         return;
    //     }
    //
    //     // Compute scale and offset factors, from pixel to ndc space. Y is inverted.
    //     //   [0, width - 2 * padding_x] to [-1, 1]
    //     //   [height - 2 * padding_y, 0] to [-1, 1]
    //     let scale_x = 2. / (width - 2. * padding_x);
    //     let scale_y = -2. / (height - 2. * padding_y);
    //     let offset_x = -1.;
    //     let offset_y = 1.;
    //
    //     info!("Width: {}, Height: {}", width, height);
    //
    //     unsafe {
    //         gl::Uniform4f(self.u_projection, offset_x, offset_y, scale_x, scale_y);
    //     }
    // }

    fn set_term_uniforms(&self, props: &term::SizeInfo) {
        unsafe {
            gl::Uniform4f(
                self.u_screen_dim,
                props.padding_x,
                props.padding_y,
                props.width,
                props.height,
            );
            gl::Uniform2f(self.u_cell_dim, props.cell_width, props.cell_height);
        }
    }
}

#[derive(Debug, Clone)]
struct GlyphRef {
    uv_left: f32,
    uv_bot: f32,
    uv_width: f32,
    uv_height: f32,
}

enum PixelFormat {
    RGBA8,
    RGB8,
    RGBA32F,
}

struct TextureFormat {
    internal: i32,
    format: u32,
    texel_type: u32,
}

fn get_gl_format(format: PixelFormat) -> TextureFormat {
    match format {
        PixelFormat::RGBA8 => TextureFormat {
            internal: gl::RGBA as i32,
            format: gl::RGBA,
            texel_type: gl::UNSIGNED_BYTE,
        },
        PixelFormat::RGB8 => TextureFormat {
            internal: gl::RGB as i32,
            format: gl::RGB,
            texel_type: gl::UNSIGNED_BYTE,
        },
        PixelFormat::RGBA32F => {
            TextureFormat { internal: gl::RGBA32F as i32, format: gl::RGBA, texel_type: gl::FLOAT }
        }
    }
}

unsafe fn upload_texture(width: i32, height: i32, format: PixelFormat, ptr: *const f32) {
    let format = get_gl_format(format);
    gl::TexImage2D(
        gl::TEXTURE_2D,
        0,
        format.internal,
        width,
        height,
        0,
        format.format,
        format.texel_type,
        ptr as *const _,
    );
}

unsafe fn create_texture(width: i32, height: i32, format: PixelFormat) -> GLuint {
    let mut id: GLuint = 0;
    let format = get_gl_format(format);

    gl::GenTextures(1, &mut id);
    gl::BindTexture(gl::TEXTURE_2D, id);
    gl::TexImage2D(
        gl::TEXTURE_2D,
        0,
        format.internal,
        width,
        height,
        0,
        format.format,
        format.texel_type,
        ptr::null(),
    );

    gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::CLAMP_TO_EDGE as i32);
    gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::CLAMP_TO_EDGE as i32);
    gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::NEAREST as i32);
    gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::NEAREST as i32);

    gl::BindTexture(gl::TEXTURE_2D, 0);
    id
}

#[derive(Debug)]
pub struct SimpleRenderer {
    // program: ScreenShaderProgram,
    // rect_program: RectShaderProgram,
    // vao: GLuint,
    // ebo: GLuint,
    // vbo_instance: GLuint,
    // rect_vao: GLuint,
    // rect_vbo: GLuint,
    atlas: Vec<Atlas>,
    current_atlas: usize,
    active_tex: GLuint,
    // batch: Batch,
    // rx: mpsc::Receiver<Msg>,
    screen_glyphs_ref: Vec<GlyphRef>,
    screen_colors_fg: Vec<[u8; 4]>,
    screen_colors_bg: Vec<[u8; 3]>,

    // Texture that stores glyph->atlas references for the entire screen
    screen_glyphs_ref_tex: GLuint,
    screen_colors_fg_tex: GLuint,
    screen_colors_bg_tex: GLuint,

    program: ScreenShaderProgram,
    vbo: GLuint,
    columns: usize,
    lines: usize,
}

impl SimpleRenderer {
    pub fn new() -> Result<SimpleRenderer, Error> {
        let screen_glyphs_ref_tex = unsafe { create_texture(256, 256, PixelFormat::RGBA32F) };
        let screen_colors_fg_tex = unsafe { create_texture(256, 256, PixelFormat::RGBA8) };
        let screen_colors_bg_tex = unsafe { create_texture(256, 256, PixelFormat::RGB8) };
        let mut vbo: GLuint = 0;

        unsafe {
            let mut vao: GLuint = 0;
            gl::GenVertexArrays(1, &mut vao);
            gl::BindVertexArray(vao);

            let vertices: [f32; 8] = [-1., 1., -1., -1., 1., 1., 1., -1.];
            gl::GenBuffers(1, &mut vbo);
            gl::BindBuffer(gl::ARRAY_BUFFER, vbo);
            gl::BufferData(
                gl::ARRAY_BUFFER,
                std::mem::size_of_val(&vertices) as isize,
                vertices.as_ptr() as *const _,
                gl::STREAM_DRAW,
            );
        }

        let mut renderer = Self {
            //     program,
            //     rect_program,
            //     vao,
            //     ebo,
            //     vbo_instance,
            //     rect_vao,
            //     rect_vbo,
            atlas: Vec::new(),
            current_atlas: 0,
            active_tex: 0,
            //     batch: Batch::new(),
            //     rx: msg_rx,
            screen_glyphs_ref: Vec::new(),
            screen_colors_fg: Vec::new(),
            screen_colors_bg: Vec::new(),

            screen_glyphs_ref_tex,
            screen_colors_fg_tex,
            screen_colors_bg_tex,
            program: ScreenShaderProgram::new()?,
            vbo,
            columns: 0,
            lines: 0,
        };

        //eprintln!("renderer: {:?}", renderer);

        let atlas = Atlas::new(ATLAS_SIZE);
        renderer.atlas.push(atlas);

        Ok(renderer)
    }

    /// Draw all rectangles simultaneously to prevent excessive program swaps.
    pub fn draw_rects(&mut self, props: &term::SizeInfo, rects: Vec<RenderRect>) {
        // Swap to rectangle rendering program.
        // unsafe {
        //     // Swap program.
        //     gl::UseProgram(self.rect_program.id);
        //
        //     // Remove padding from viewport.
        //     gl::Viewport(0, 0, props.width as i32, props.height as i32);
        //
        //     // Change blending strategy.
        //     gl::BlendFuncSeparate(gl::SRC_ALPHA, gl::ONE_MINUS_SRC_ALPHA, gl::SRC_ALPHA, gl::ONE);
        //
        //     // Setup data and buffers.
        //     gl::BindVertexArray(self.rect_vao);
        //     gl::BindBuffer(gl::ARRAY_BUFFER, self.rect_vbo);
        //
        //     // Position.
        //     gl::VertexAttribPointer(
        //         0,
        //         2,
        //         gl::FLOAT,
        //         gl::FALSE,
        //         (size_of::<f32>() * 2) as _,
        //         ptr::null(),
        //     );
        //     gl::EnableVertexAttribArray(0);
        // }

        // Draw all the rects.
        // for rect in rects {
        //     self.render_rect(&rect, props);
        // }
        //
        // // Deactivate rectangle program again.
        // unsafe {
        //     // Reset blending strategy.
        //     gl::BlendFunc(gl::SRC1_COLOR, gl::ONE_MINUS_SRC1_COLOR);
        //
        //     // Reset data and buffers.
        //     gl::BindBuffer(gl::ARRAY_BUFFER, 0);
        //     gl::BindVertexArray(0);
        //
        //     let padding_x = props.padding_x as i32;
        //     let padding_y = props.padding_y as i32;
        //     let width = props.width as i32;
        //     let height = props.height as i32;
        //     gl::Viewport(padding_x, padding_y, width - 2 * padding_x, height - 2 * padding_y);
        //
        //     // Disable program.
        //     gl::UseProgram(0);
        // }
    }

    pub fn with_api<F, T, C>(&mut self, config: &Config<C>, props: &term::SizeInfo, func: F) -> T
    where
        F: FnOnce(RenderApi<'_, C>) -> T,
    {
        //eprintln!("clear");
        self.screen_glyphs_ref
            .iter_mut()
            .map(|x| *x = GlyphRef { uv_bot: 0.0, uv_left: 0.0, uv_width: 0.0, uv_height: 0.0 })
            .count();

        self.screen_colors_fg.iter_mut().map(|x| *x = [0u8; 4]).count();
        self.screen_colors_bg.iter_mut().map(|x| *x = [0u8; 3]).count();

        let res = func(RenderApi {
            seen_cells: false,
            this: self,
            props,
            // active_tex: &mut self.active_tex,
            // // batch: &mut self.batch,
            // atlas: &mut self.atlas,
            // current_atlas: &mut self.current_atlas,
            // program: &mut self.program,
            config,
            // vbo: self.vbo,
            // screen_glyphs_ref: &mut self.screen_glyphs_ref,
            // screen_glyphs_ref_tex: self.screen_glyphs_ref_tex,
            // screen_colors_fg: self.screen_colors
            // screen_colors_fg_tex,
            // screen_colors_bg_tex,
            // columns: self.columns,
            // lines: self.lines,
        });

        // unsafe {
        //     gl::BindBuffer(gl::ELEMENT_ARRAY_BUFFER, 0);
        //     gl::BindBuffer(gl::ARRAY_BUFFER, 0);
        //     gl::BindVertexArray(0);
        //
        //     gl::UseProgram(0);
        // }

        res
    }

    pub fn with_loader<F, T>(&mut self, func: F) -> T
    where
        F: FnOnce(LoaderApi<'_>) -> T,
    {
        unsafe {
            gl::ActiveTexture(gl::TEXTURE0);
        }

        func(LoaderApi {
            active_tex: &mut self.active_tex,
            atlas: &mut self.atlas,
            current_atlas: &mut self.current_atlas,
        })
    }

    // pub fn reload_shaders(&mut self, props: &term::SizeInfo) {
    //     info!("Reloading shaders...");
    //     let result = (ScreenShaderProgram::new(), RectShaderProgram::new());
    //     let (program, rect_program) = match result {
    //         (Ok(program), Ok(rect_program)) => {
    //             unsafe {
    //                 gl::UseProgram(program.id);
    //                 program.update_projection(
    //                     props.width,
    //                     props.height,
    //                     props.padding_x,
    //                     props.padding_y,
    //                 );
    //                 gl::UseProgram(0);
    //             }
    //
    //             info!("... successfully reloaded shaders");
    //             (program, rect_program)
    //         }
    //         (Err(err), _) | (_, Err(err)) => {
    //             error!("{}", err);
    //             return;
    //         }
    //     };
    //
    //     self.active_tex = 0;
    //     self.program = program;
    //     self.rect_program = rect_program;
    // }

    pub fn resize(&mut self, size: &term::SizeInfo) {
        // Viewport.
        unsafe {
            gl::Viewport(
                size.padding_x as i32,
                size.padding_y as i32,
                size.width as i32 - 2 * size.padding_x as i32,
                size.height as i32 - 2 * size.padding_y as i32,
            );

            // Update projection.
            // gl::UseProgram(self.program.id);
            // self.program.update_projection(size.width, size.height, size.padding_x, size.padding_y);
            // gl::UseProgram(0);
        }

        self.columns = size.cols().0;
        self.lines = size.lines().0;
        let cells = self.columns * self.lines;

        self.screen_colors_bg.resize(cells, [0u8; 3]);
        self.screen_colors_fg.resize(cells, [0u8; 4]);
        self.screen_glyphs_ref
            .resize(cells, GlyphRef { uv_bot: 0.0, uv_left: 0.0, uv_width: 0.0, uv_height: 0.0 });
    }

    /// Render a rectangle.
    ///
    /// This requires the rectangle program to be activated.
    fn render_rect(&mut self, rect: &RenderRect, size: &term::SizeInfo) {
        // Do nothing when alpha is fully transparent.
        if rect.alpha == 0. {
            return;
        }

        // Calculate rectangle position.
        let center_x = size.width / 2.;
        let center_y = size.height / 2.;
        let x = (rect.x - center_x) / center_x;
        let y = -(rect.y - center_y) / center_y;
        let width = rect.width / center_x;
        let height = rect.height / center_y;

        // unsafe {
        //     // Setup vertices.
        //     let vertices: [f32; 8] = [x + width, y, x + width, y - height, x, y - height, x, y];
        //
        //     // Load vertex data into array buffer.
        //     gl::BufferData(
        //         gl::ARRAY_BUFFER,
        //         (size_of::<f32>() * vertices.len()) as _,
        //         vertices.as_ptr() as *const _,
        //         gl::STATIC_DRAW,
        //     );
        //
        //     // Color.
        //     self.rect_program.set_color(rect.color, rect.alpha);
        //
        //     // Draw the rectangle.
        //     gl::DrawElements(gl::TRIANGLES, 6, gl::UNSIGNED_INT, ptr::null());
        // }
    }

    fn render(&mut self, props: &SizeInfo) {
        //eprintln!("render");
        #[cfg(feature = "live-shader-reload")]
        {
            match self.program.poll() {
                Err(e) => {
                    error!("shader error: {}", e);
                }
                Ok(updated) if updated => {
                    eprintln!("updated shader: {:?}", self.program);
                }
                _ => {}
            }
        }

        unsafe {
            gl::UseProgram(self.program.program.id);

            self.program.set_term_uniforms(props);
            gl::Uniform1i(self.program.u_atlas, 0);
            gl::Uniform1i(self.program.u_glyph_ref, 1);
            gl::Uniform1i(self.program.u_color_fg, 2);
            gl::Uniform1i(self.program.u_color_bg, 3);

            gl::BindTexture(gl::TEXTURE_2D, self.atlas[self.current_atlas].id);

            gl::ActiveTexture(gl::TEXTURE1);
            gl::BindTexture(gl::TEXTURE_2D, self.screen_glyphs_ref_tex);
            //eprintln!("glyphs: {:?}", &self.screen_glyphs_ref[0..10]);
            upload_texture(
                self.columns as i32,
                self.lines as i32,
                PixelFormat::RGBA32F,
                self.screen_glyphs_ref.as_ptr() as *const _,
            );

            gl::ActiveTexture(gl::TEXTURE2);
            gl::BindTexture(gl::TEXTURE_2D, self.screen_colors_fg_tex);
            upload_texture(
                self.columns as i32,
                self.lines as i32,
                PixelFormat::RGBA8,
                self.screen_colors_fg.as_ptr() as *const _,
            );

            gl::ActiveTexture(gl::TEXTURE3);
            gl::BindTexture(gl::TEXTURE_2D, self.screen_colors_bg_tex);
            upload_texture(
                self.columns as i32,
                self.lines as i32,
                PixelFormat::RGB8,
                self.screen_colors_bg.as_ptr() as *const _,
            );

            gl::BindBuffer(gl::ARRAY_BUFFER, self.vbo);
            gl::VertexAttribPointer(0, 2, gl::FLOAT, gl::FALSE, 0, ptr::null());
            gl::EnableVertexAttribArray(0);

            gl::DrawArrays(gl::TRIANGLE_STRIP, 0, 4);
            gl::DisableVertexAttribArray(0);
            gl::ActiveTexture(gl::TEXTURE0);
        }
    }
}

#[derive(Debug)]
pub struct RenderApi<'a, C> {
    seen_cells: bool,
    this: &'a mut SimpleRenderer,
    //active_tex: &'a mut GLuint,
    props: &'a term::SizeInfo,
    // atlas: &'a mut Vec<Atlas>,
    // current_atlas: &'a mut usize,
    // program: &'a mut ScreenShaderProgram,
    // screen_glyphs_ref_tex: GLuint,
    // screen_glyphs_ref: &'a mut Vec<GlyphRef>,
    config: &'a Config<C>,
    // columns: usize,
    // lines: usize,
    // vbo: GLuint,
}

impl<'a, C> RenderApi<'a, C> {
    pub fn clear(&self, color: Rgb) {
        debug!("clear");
        unsafe {
            let alpha = self.config.background_opacity();
            gl::ClearColor(
                (f32::from(color.r) / 255.0).min(1.0) * alpha,
                (f32::from(color.g) / 255.0).min(1.0) * alpha,
                (f32::from(color.b) / 255.0).min(1.0) * alpha,
                alpha,
            );
            gl::Clear(gl::COLOR_BUFFER_BIT);
        }
    }

    #[cfg(not(any(target_os = "macos", windows)))]
    pub fn finish(&self) {
        unsafe {
            gl::Finish();
        }
    }

    fn render_batch(&mut self) {
        debug!("render_batch");
        // unsafe {
        //     gl::BufferSubData(
        //         gl::ARRAY_BUFFER,
        //         0,
        //         self.batch.size() as isize,
        //         self.batch.instances.as_ptr() as *const _,
        //     );
        // }
        //
        // // Bind texture if necessary.
        // if *self.active_tex != self.batch.tex {
        //     unsafe {
        //         gl::BindTexture(gl::TEXTURE_2D, self.batch.tex);
        //     }
        //     *self.active_tex = self.batch.tex;
        // }
        //
        // unsafe {
        //     self.program.set_background_pass(true);
        //     gl::DrawElementsInstanced(
        //         gl::TRIANGLES,
        //         6,
        //         gl::UNSIGNED_INT,
        //         ptr::null(),
        //         self.batch.len() as GLsizei,
        //     );
        //     self.program.set_background_pass(false);
        //     gl::DrawElementsInstanced(
        //         gl::TRIANGLES,
        //         6,
        //         gl::UNSIGNED_INT,
        //         ptr::null(),
        //         self.batch.len() as GLsizei,
        //     );
        // }
        //
        // self.batch.clear();
    }

    /// Render a string in a variable location. Used for printing the render timer, warnings and
    /// errors.
    pub fn render_string(
        &mut self,
        string: &str,
        line: Line,
        glyph_cache: &mut GlyphCache,
        color: Option<Rgb>,
    ) {
        // let bg_alpha = color.map(|_| 1.0).unwrap_or(0.0);
        // let col = Column(0);
        //
        // let cells = string
        //     .chars()
        //     .enumerate()
        //     .map(|(i, c)| RenderableCell {
        //         line,
        //         column: col + i,
        //         inner: RenderableCellContent::Chars({
        //             let mut chars = [' '; cell::MAX_ZEROWIDTH_CHARS + 1];
        //             chars[0] = c;
        //             chars
        //         }),
        //         bg: color.unwrap_or(Rgb { r: 0, g: 0, b: 0 }),
        //         fg: Rgb { r: 0, g: 0, b: 0 },
        //         flags: Flags::empty(),
        //         bg_alpha,
        //     })
        //     .collect::<Vec<_>>();
        //
        // for cell in cells {
        //     self.render_cell(cell, glyph_cache);
        // }
    }
    //
    // #[inline]
    // fn add_render_item(&mut self, cell: RenderableCell, glyph: &Glyph) {
    //     // Flush batch if tex changing.
    //     if !self.batch.is_empty() && self.batch.tex != glyph.tex_id {
    //         self.render_batch();
    //     }
    //
    //     self.batch.add_item(cell, glyph);
    //
    //     // Render batch and clear if it's full.
    //     if self.batch.full() {
    //         self.render_batch();
    //     }
    // }

    // #[inline]
    // fn update_main_texture_cell(&mut self, cell: RenderableCell, glyph: &Glyph) {}

    pub fn render_cell(&mut self, cell: RenderableCell, glyph_cache: &mut GlyphCache) {
        self.seen_cells = true;
        let chars = match cell.inner {
            RenderableCellContent::Cursor(cursor_key) => {
                // Raw cell pixel buffers like cursors don't need to go through font lookup.
                // let metrics = glyph_cache.metrics;
                // let glyph = glyph_cache.cursor_cache.entry(cursor_key).or_insert_with(|| {
                //     self.load_glyph(&cursor::get_cursor_glyph(
                //         cursor_key.style,
                //         metrics,
                //         self.config.font.offset.x,
                //         self.config.font.offset.y,
                //         cursor_key.is_wide,
                //         self.config.cursor.thickness(),
                //     ))
                // });
                // self.add_render_item(cell, glyph);
                debug!("lol cursor @{},{}", cell.line, cell.column);
                return;
            }
            RenderableCellContent::Chars(chars) => chars,
        };

        // Get font key for cell.
        let font_key = match cell.flags & Flags::BOLD_ITALIC {
            Flags::BOLD_ITALIC => glyph_cache.bold_italic_key,
            Flags::ITALIC => glyph_cache.italic_key,
            Flags::BOLD => glyph_cache.bold_key,
            _ => glyph_cache.font_key,
        };

        // Don't render text of HIDDEN cells.
        let mut chars = if cell.flags.contains(Flags::HIDDEN) {
            [' '; cell::MAX_ZEROWIDTH_CHARS + 1]
        } else {
            chars
        };

        // Render tabs as spaces in case the font doesn't support it.
        if chars[0] == '\t' {
            chars[0] = ' ';
        }

        let mut glyph_key = GlyphKey { font_key, size: glyph_cache.font_size, c: chars[0] };

        // Add cell to batch.
        let glyph: Glyph = *glyph_cache.get(glyph_key, self);
        let cell_index = cell.line.0 * self.this.columns + cell.column.0;

        // put glyph reference into texture data
        self.this.screen_glyphs_ref[cell_index] = GlyphRef {
            uv_bot: glyph.uv_bot,
            uv_left: glyph.uv_left,
            uv_width: glyph.uv_width,
            uv_height: glyph.uv_height,
        };
        //eprintln!("{}: {:?}", cell_index, self.this.screen_glyphs_ref[cell_index]);

        self.this.screen_colors_fg[cell_index] =
            [cell.fg.r, cell.fg.g, cell.fg.b, (cell.bg_alpha * 255.0) as u8];

        self.this.screen_colors_bg[cell_index] = [cell.bg.r, cell.bg.g, cell.bg.b];

        // // Render zero-width characters.
        // for c in (&chars[1..]).iter().filter(|c| **c != ' ') {
        //     glyph_key.c = *c;
        //     let mut glyph = *glyph_cache.get(glyph_key, self);
        //
        //     // The metrics of zero-width characters are based on rendering
        //     // the character after the current cell, with the anchor at the
        //     // right side of the preceding character. Since we render the
        //     // zero-width characters inside the preceding character, the
        //     // anchor has been moved to the right by one cell.
        //     glyph.left += glyph_cache.metrics.average_advance as f32;
        //
        //     self.add_render_item(cell, &glyph);
        // }
    }
}

impl<'a, C> LoadGlyph for RenderApi<'a, C> {
    fn load_glyph(&mut self, rasterized: &RasterizedGlyph) -> Glyph {
        load_glyph(
            &mut self.this.active_tex,
            &mut self.this.atlas,
            &mut self.this.current_atlas,
            rasterized,
        )
    }

    fn clear(&mut self) {
        clear_atlas(&mut self.this.atlas, &mut self.this.current_atlas)
    }
}

impl<'a, C> Drop for RenderApi<'a, C> {
    fn drop(&mut self) {
        if self.seen_cells {
            self.this.render(self.props);
        }
    }
}
