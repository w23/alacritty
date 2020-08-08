use crate::cursor;
use {
    crate::{
        config::{
            font::{Font, FontDescription},
            ui_config::{Delta, UIConfig},
            window::{StartupMode, WindowConfig},
        },
        gl,
        gl::types::*,
        renderer::{
            clear_atlas, create_program, filewatch, get_shader_info_log, rects::RenderRect, Atlas,
            Error, Glyph, GlyphCache, LoadGlyph, ShaderCreationError, ATLAS_SIZE,
        },
    },
    alacritty_terminal::{
        index::{Column, Line},
        term::{
            self,
            cell::{self, Flags, MAX_ZEROWIDTH_CHARS},
            color::Rgb,
            CursorKey, RenderableCell, RenderableCellContent, SizeInfo,
        },
    },
    log::*,
    std::{mem::size_of, path::PathBuf, ptr},
};

use crossfont::{
    BitmapBuffer, FontDesc, FontKey, GlyphKey, Rasterize, RasterizedGlyph, Rasterizer, Size, Slant,
    Style, Weight,
};

use alacritty_terminal::config::Cursor;

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
    u_cursor: GLint,
    u_cursor_color: GLint,

    u_atlas: GLint,
}

static SCREEN_SHADER_F_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/res/screen.f.glsl");
static SCREEN_SHADER_V_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/res/screen.v.glsl");
static SCREEN_SHADER_F: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/res/screen.f.glsl"));
static SCREEN_SHADER_V: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/res/screen.v.glsl"));

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
            u_cursor: -1,
            u_cursor_color: -1,
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
            u_cursor: -1,
            u_cursor_color: -1,
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
        let (screen_dim, cell_dim, atlas, color_bg, color_fg, glyph_ref, cursor, cursor_color) = unsafe {
            (
                gl::GetUniformLocation(self.program.id, cptr!(b"screenDim\0")),
                gl::GetUniformLocation(self.program.id, cptr!(b"cellDim\0")),
                gl::GetUniformLocation(self.program.id, cptr!(b"atlas\0")),
                gl::GetUniformLocation(self.program.id, cptr!(b"color_bg\0")),
                gl::GetUniformLocation(self.program.id, cptr!(b"color_fg\0")),
                gl::GetUniformLocation(self.program.id, cptr!(b"glyphRef\0")),
                gl::GetUniformLocation(self.program.id, cptr!(b"cursor\0")),
                gl::GetUniformLocation(self.program.id, cptr!(b"cursor_color\0")),
            )
        };

        if validate_uniforms {
            assert_uniform_valid!(
                screen_dim,
                cell_dim,
                atlas,
                color_bg,
                color_fg,
                glyph_ref,
                cursor,
                cursor_color
            );
        }

        self.u_screen_dim = screen_dim;
        self.u_cell_dim = cell_dim;
        self.u_glyph_ref = glyph_ref;
        self.u_atlas = atlas;
        self.u_color_fg = color_fg;
        self.u_color_bg = color_bg;
        self.u_cursor = cursor;
        self.u_cursor_color = cursor_color;
    }

    #[cfg(feature = "live-shader-reload")]
    fn poll(&mut self) -> Result<bool, ShaderCreationError> {
        if self.program.poll()? {
            self.update(false);
            return Ok(true);
        }

        Ok(false)
    }

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
    x: u8,
    y: u8,
    z: u8,
    w: u8,
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

    gl::PixelStorei(gl::UNPACK_ALIGNMENT, 1);

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

// TODO figure out dynamically based on GL caps
static GRID_ATLAS_SIZE: i32 = 1024;

#[derive(Debug)]
enum AtlasError {
    TooBig { w: i32, h: i32, cw: i32, ch: i32 },
    Full,
}

#[derive(Debug)]
struct GridAtlas {
    tex: GLuint,
    cell_width: i32,
    cell_height: i32,
    grid_width: i32,
    grid_height: i32,
    free_line: i32,
    free_column: i32,
}

impl GridAtlas {
    fn new(props: &term::SizeInfo) -> Self {
        // FIXME limit atlas size by 256x256 cells
        let cell_width = props.cell_width as i32;
        let cell_height = props.cell_height as i32;
        Self {
            tex: unsafe { create_texture(GRID_ATLAS_SIZE, GRID_ATLAS_SIZE, PixelFormat::RGBA8) },
            grid_width: GRID_ATLAS_SIZE / cell_width,
            grid_height: GRID_ATLAS_SIZE / cell_height,
            cell_width,
            cell_height,
            free_line: 0,
            free_column: 1,
        }
    }

    fn load_glyph(&mut self, rasterized: &RasterizedGlyph) -> Result<Glyph, AtlasError> {
        if rasterized.width > self.cell_width || rasterized.height > self.cell_height {
            eprintln!(
                "{} {},{} {}x{}",
                rasterized.c, rasterized.left, rasterized.top, rasterized.width, rasterized.height,
            );

            // return Err(AtlasError::TooBig {
            //     w: rasterized.width,
            //     h: rasterized.height,
            //     cw: self.cell_width,
            //     ch: self.cell_height,
            // });
        }

        if self.free_line >= self.cell_height {
            return Err(AtlasError::Full);
        }

        let colored;
        let line = self.free_line;
        let column = self.free_column;

        unsafe {
            gl::BindTexture(gl::TEXTURE_2D, self.tex);

            // Load data into OpenGL.
            let (format, buf) = match &rasterized.buf {
                BitmapBuffer::RGB(buf) => {
                    colored = false;
                    (gl::RGB, buf)
                }
                BitmapBuffer::RGBA(buf) => {
                    colored = true;
                    (gl::RGBA, buf)
                }
            };

            // TODO optimize
            // 1. only copy into internal storage
            // 2. upload once before drawing by column/line subrect
            gl::TexSubImage2D(
                gl::TEXTURE_2D,
                0,
                std::cmp::max(0, column * self.cell_width + rasterized.left),
                std::cmp::max(0, line * self.cell_height + self.cell_height - rasterized.top),
                rasterized.width,
                rasterized.height,
                format,
                gl::UNSIGNED_BYTE,
                buf.as_ptr() as *const _,
            );

            gl::BindTexture(gl::TEXTURE_2D, 0);
        }

        // eprintln!(
        //     "{} {},{} {}x{} => l={} c={}",
        //     rasterized.c,
        //     rasterized.left,
        //     rasterized.top,
        //     rasterized.width,
        //     rasterized.height,
        //     line,
        //     column
        // );

        self.free_column += 1;
        if self.free_column == self.grid_width {
            self.free_column = 0;
            self.free_line += 1;
        }

        // TODO make Glyph enum
        Ok(Glyph {
            tex_id: self.tex,
            colored,
            top: 0.0,
            left: 0.0,
            width: 0.0,
            height: 0.0,
            uv_bot: line as f32,
            uv_left: column as f32,
            uv_width: 0.0,
            uv_height: 0.0,
        })
    }
}

impl Drop for GridAtlas {
    fn drop(&mut self) {
        unsafe {
            gl::DeleteTextures(1, &mut self.tex);
        }
    }
}

#[derive(Debug)]
pub struct SimpleRenderer {
    atlas: Option<GridAtlas>,
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

    cursor_cell: [f32; 2],
    cursor_glyph: [f32; 2],
    cursor_color: Rgb,
}

impl SimpleRenderer {
    pub fn new() -> Result<SimpleRenderer, Error> {
        let screen_glyphs_ref_tex = unsafe { create_texture(256, 256, PixelFormat::RGBA8) };
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
            atlas: None,
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

            cursor_cell: [-1.0; 2],
            cursor_glyph: [-1.0; 2],
            cursor_color: Rgb { r: 0, g: 0, b: 0 },
        };

        //eprintln!("renderer: {:?}", renderer);

        Ok(renderer)
    }

    pub fn set_cursor(
        &mut self,
        column: usize,
        line: usize,
        glyph_x: f32,
        glyph_y: f32,
        color: Rgb,
    ) {
        self.cursor_cell = [column as f32, line as f32];
        self.cursor_glyph = [glyph_x, glyph_y];
        self.cursor_color = color;
    }

    /// Draw all rectangles simultaneously to prevent excessive program swaps.
    pub fn draw_rects(&mut self, props: &term::SizeInfo, rects: Vec<RenderRect>) {
        //error!("draw_rects is not implemented");
    }

    pub fn with_api<F, T>(
        &mut self,
        config: &UIConfig,
        cursor_config: Cursor,
        props: &SizeInfo,
        func: F,
    ) -> T
    where
        F: FnOnce(RenderApi<'_>) -> T,
    {
        func(RenderApi { seen_cells: false, this: self, props, config, cursor_config })
    }

    pub fn with_loader<F, T>(&mut self, func: F) -> T
    where
        F: FnOnce(LoaderApi<'_>) -> T,
    {
        unsafe {
            gl::ActiveTexture(gl::TEXTURE0);
        }

        func(LoaderApi { renderer: self })
    }

    pub fn resize(&mut self, size: &term::SizeInfo) {
        // Viewport.
        unsafe {
            gl::Viewport(
                size.padding_x as i32,
                size.padding_y as i32,
                size.width as i32 - 2 * size.padding_x as i32,
                size.height as i32 - 2 * size.padding_y as i32,
            );
        }

        self.columns = size.cols().0;
        self.lines = size.lines().0;
        let cells = self.columns * self.lines;

        self.screen_colors_bg.resize(cells, [0u8; 3]);
        self.screen_colors_fg.resize(cells, [0u8; 4]);
        self.screen_glyphs_ref.resize(cells, GlyphRef { x: 0, y: 0, z: 0, w: 0 });
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
            gl::Uniform4f(
                self.program.u_cursor,
                self.cursor_cell[0],
                self.cursor_cell[1],
                self.cursor_glyph[0],
                self.cursor_glyph[1],
            );
            gl::Uniform3f(
                self.program.u_cursor_color,
                self.cursor_color.r as f32 / 255.,
                self.cursor_color.g as f32 / 255.,
                self.cursor_color.b as f32 / 255.,
            );

            gl::BindTexture(gl::TEXTURE_2D, self.atlas.as_ref().unwrap().tex);

            gl::ActiveTexture(gl::TEXTURE1);
            gl::BindTexture(gl::TEXTURE_2D, self.screen_glyphs_ref_tex);
            //eprintln!("glyphs: {:?}", &self.screen_glyphs_ref[0..10]);
            upload_texture(
                self.columns as i32,
                self.lines as i32,
                PixelFormat::RGBA8,
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

    fn load_glyph(&mut self, rasterized: &RasterizedGlyph) -> Glyph {
        if self.atlas.is_some() {
            match self.atlas.as_mut().unwrap().load_glyph(rasterized) {
                Err(e) => {
                    error!("{:?}: {}", e, rasterized.c);
                }
                Ok(glyph) => {
                    return glyph;
                }
            }
        }

        Glyph {
            tex_id: 0,
            colored: false,
            top: 0.0,
            left: 0.0,
            width: 0.0,
            height: 0.0,
            uv_bot: 0.0,
            uv_left: 0.0,
            uv_width: 0.0,
            uv_height: 0.0,
        }
    }

    fn clear_atlas(&mut self) {
        self.atlas = None;
    }
}

#[derive(Debug)]
pub struct RenderApi<'a> {
    seen_cells: bool,
    this: &'a mut SimpleRenderer,
    props: &'a term::SizeInfo,
    config: &'a UIConfig,
    cursor_config: Cursor,
}

impl<'a> RenderApi<'a> {
    pub fn clear(&mut self, color: Rgb) {
        self.this
            .screen_glyphs_ref
            .iter_mut()
            .map(|x| *x = GlyphRef { x: 0, y: 0, z: 0, w: 0 })
            .count();
        self.this.screen_colors_fg.iter_mut().map(|x| *x = [0u8; 4]).count();
        self.this.screen_colors_bg.iter_mut().map(|x| *x = [color.r, color.g, color.b]).count();

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

    /// Render a string in a variable location. Used for printing the render timer, warnings and
    /// errors.
    pub fn render_string(
        &mut self,
        glyph_cache: &mut GlyphCache,
        line: Line,
        string: &str,
        fg: Rgb,
        bg: Option<Rgb>,
    ) {
        //error!("render_string({}) not implemented", string);
    }

    pub fn render_cell(&mut self, cell: RenderableCell, glyph_cache: &mut GlyphCache) {
        self.seen_cells = true;

        if self.this.atlas.is_none() {
            self.this.atlas = Some(GridAtlas::new(self.props));
        }

        let glyph = match cell.inner {
            RenderableCellContent::Cursor(cursor_key) => {
                // Raw cell pixel buffers like cursors don't need to go through font lookup.
                let metrics = glyph_cache.metrics;
                let glyph = glyph_cache.cursor_cache.entry(cursor_key).or_insert_with(|| {
                    self.load_glyph(&cursor::get_cursor_glyph(
                        cursor_key.style,
                        metrics,
                        self.config.font.offset.x,
                        self.config.font.offset.y,
                        cursor_key.is_wide,
                        self.cursor_config.thickness(),
                    ))
                });
                // self.add_render_item(cell, glyph);
                //eprintln!("???? lol cursor @{},{} => {:?}", cell.line, cell.column, glyph);
                self.this.set_cursor(
                    cell.column.0,
                    cell.line.0,
                    glyph.uv_left,
                    glyph.uv_bot,
                    cell.fg,
                );
                return;
            }
            RenderableCellContent::Chars(chars) => {
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

                let glyph_key = GlyphKey { font_key, size: glyph_cache.font_size, c: chars[0] };
                glyph_cache.get(glyph_key, self)
            }
        };

        let cell_index = cell.line.0 * self.this.columns + cell.column.0;

        //eprintln!("{},{} {:?}", cell.line.0, cell.column.0, glyph);

        // put glyph reference into texture data
        self.this.screen_glyphs_ref[cell_index] = GlyphRef {
            x: glyph.uv_left as u8,
            y: glyph.uv_bot as u8,
            z: glyph.colored as u8,
            w: 0,
        };
        // eprintln!(
        //     "{},{} -> {}: {:?}",
        //     cell.line.0, cell.column.0, cell_index, self.this.screen_glyphs_ref[cell_index]
        // );

        self.this.screen_colors_fg[cell_index] =
            [cell.fg.r, cell.fg.g, cell.fg.b, (cell.bg_alpha * 255.0) as u8];

        self.this.screen_colors_bg[cell_index] = [cell.bg.r, cell.bg.g, cell.bg.b];

        // FIXME Render zero-width characters.
        // FIXME Ligatures? How do they work?
    }
}

impl<'a> LoadGlyph for RenderApi<'a> {
    fn load_glyph(&mut self, rasterized: &RasterizedGlyph) -> Glyph {
        self.this.load_glyph(rasterized)
    }

    fn clear(&mut self) {
        self.this.clear_atlas();
    }
}

impl<'a> Drop for RenderApi<'a> {
    fn drop(&mut self) {
        if self.seen_cells {
            self.this.render(self.props);
        }
    }
}

#[derive(Debug)]
pub struct LoaderApi<'a> {
    renderer: &'a mut SimpleRenderer,
}

impl<'a> LoadGlyph for LoaderApi<'a> {
    fn load_glyph(&mut self, rasterized: &RasterizedGlyph) -> Glyph {
        self.renderer.load_glyph(rasterized)
    }

    fn clear(&mut self) {
        self.renderer.clear_atlas();
    }
}
