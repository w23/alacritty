use crate::gl;
use crate::gl::types::*;
use alacritty_terminal::term::SizeInfo;
use std::fmt;
use std::fmt::Display;
use std::fmt::Formatter;
use std::fs;
use std::io;
use std::path::PathBuf;

#[cfg(feature = "live-shader-reload")]
use super::filewatch;

#[derive(Debug)]
pub enum ShaderCreationError {
    /// Error reading file.
    Io(io::Error),

    /// Error compiling shader.
    Compile(PathBuf, String),

    /// Problem linking.
    Link(String),
}

impl std::error::Error for ShaderCreationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ShaderCreationError::Io(err) => err.source(),
            _ => None,
        }
    }
}

impl Display for ShaderCreationError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            ShaderCreationError::Io(err) => write!(f, "Unable to read shader: {}", err),
            ShaderCreationError::Compile(path, log) => {
                write!(f, "Failed compiling shader at {}: {}", path.display(), log)
            },
            ShaderCreationError::Link(log) => write!(f, "Failed linking shader: {}", log),
        }
    }
}

impl From<io::Error> for ShaderCreationError {
    fn from(val: io::Error) -> Self {
        ShaderCreationError::Io(val)
    }
}

fn create_shader_from_source(kind: GLenum, source: &str) -> Result<GLuint, ShaderCreationError> {
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

fn create_program(vertex: GLuint, fragment: GLuint) -> Result<GLuint, ShaderCreationError> {
    unsafe {
        let program = gl::CreateProgram();
        gl::AttachShader(program, vertex);
        gl::AttachShader(program, fragment);
        gl::LinkProgram(program);

        let mut success: GLint = 0;
        gl::GetProgramiv(program, gl::LINK_STATUS, &mut success);

        if success == i32::from(gl::TRUE) {
            Ok(program)
        } else {
            Err(ShaderCreationError::Link(get_program_info_log(program)))
        }
    }
}

fn get_shader_info_log(shader: GLuint) -> String {
    // Get expected log length.
    let mut max_length: GLint = 0;
    unsafe {
        gl::GetShaderiv(shader, gl::INFO_LOG_LENGTH, &mut max_length);
    }

    // Read the info log.
    let mut actual_length: GLint = 0;
    let mut buf: Vec<u8> = Vec::with_capacity(max_length as usize);
    unsafe {
        gl::GetShaderInfoLog(shader, max_length, &mut actual_length, buf.as_mut_ptr() as *mut _);
    }

    // Build a string.
    unsafe {
        buf.set_len(actual_length as usize);
    }

    // XXX should we expect OpenGL to return garbage?
    String::from_utf8(buf).unwrap()
}

fn get_program_info_log(program: GLuint) -> String {
    // Get expected log length.
    let mut max_length: GLint = 0;
    unsafe {
        gl::GetProgramiv(program, gl::INFO_LOG_LENGTH, &mut max_length);
    }

    // Read the info log.
    let mut actual_length: GLint = 0;
    let mut buf: Vec<u8> = Vec::with_capacity(max_length as usize);
    unsafe {
        gl::GetProgramInfoLog(program, max_length, &mut actual_length, buf.as_mut_ptr() as *mut _);
    }

    // Build a string.
    unsafe {
        buf.set_len(actual_length as usize);
    }

    // XXX should we expect OpenGL to return garbage?
    String::from_utf8(buf).unwrap()
}

macro_rules! cptr {
    ($thing:expr) => {
        $thing.as_ptr() as *const _
    };
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

    #[cfg(feature = "live-shader-reload")]
    fn valid(&self) -> bool {
        self.id != 0
    }

    #[cfg(feature = "live-shader-reload")]
    fn poll(&mut self) -> Result<bool, ShaderCreationError> {
        Ok(match self.file.read_update() {
            Some(src) => {
                let new_id = create_shader_from_source(self.kind, &src)?;
                self.delete();
                self.id = new_id;
                true
            },
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
    /// OpenGL program id
    id: GLuint,

    #[cfg(feature = "live-shader-reload")]
    vertex_shader: Shader,

    #[cfg(feature = "live-shader-reload")]
    fragment_shader: Shader,
}

impl ShaderProgram {
    #[cfg(not(feature = "live-shader-reload"))]
    fn from_sources(vertex_src: &str, fragment_src: &str) -> Result<Self, ShaderCreationError> {
        let vertex_shader = create_shader_from_source(gl::VERTEX_SHADER, vertex_src)?;
        let fragment_shader = create_shader_from_source(gl::FRAGMENT_SHADER, fragment_src)?;
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

static RECT_SHADER_V_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/res/rect.v.glsl");
static RECT_SHADER_F_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/res/rect.f.glsl");
#[cfg(feature = "live-shader-reload")]
static SCREEN_SHADER_V_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/res/screen.v.glsl");
#[cfg(feature = "live-shader-reload")]
static SCREEN_SHADER_F_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/res/screen.f.glsl");
#[cfg(feature = "live-shader-reload")]
static GLYPHRECT_SHADER_V_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/res/glyphrect.v.glsl");
#[cfg(feature = "live-shader-reload")]
static GLYPHRECT_SHADER_F_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/res/glyphrect.f.glsl");

// Shader source which is used when live-shader-reload feature is disabled.
static RECT_SHADER_V: &str = include_str!("../../res/rect.v.glsl");
static RECT_SHADER_F: &str = include_str!("../../res/rect.f.glsl");
#[cfg(not(feature = "live-shader-reload"))]
static GLYPHRECT_SHADER_V: &str = include_str!("../../res/glyphrect.v.glsl");
#[cfg(not(feature = "live-shader-reload"))]
static GLYPHRECT_SHADER_F: &str = include_str!("../../res/glyphrect.f.glsl");
#[cfg(not(feature = "live-shader-reload"))]
static SCREEN_SHADER_V: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/res/screen.v.glsl"));
#[cfg(not(feature = "live-shader-reload"))]
static SCREEN_SHADER_F: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/res/screen.f.glsl"));

macro_rules! declare_program {
	($struct:ident, $vpath:ident, $vsrc:ident, $fpath:ident, $fsrc:ident {$( $uniform:ident ),*}) => {
	  #[derive(Debug)]
		pub struct $struct {
			program: ShaderProgram,
			$(
				pub $uniform: GLint,
			)*
		}

		impl $struct {
			#[cfg(feature = "live-shader-reload")]
			pub fn new() -> Result<Self, ShaderCreationError> {
				Ok(Self {
						program: ShaderProgram::from_files($vpath, $fpath)?,
					$(
						$uniform: -1,
					)*
				})
			}

			#[cfg(not(feature = "live-shader-reload"))]
			pub fn new() -> Result<Self, ShaderCreationError> {
				let mut this = Self {
						program: ShaderProgram::from_sources($vsrc, $fsrc)?,
					$(
						$uniform: -1,
					)*
				};
        this.update(true);
				Ok(this)
			}

			pub fn get_id(&self) -> GLuint {
				self.program.id
			}

			fn update(&mut self, validate_uniforms: bool) {
				$(
					 self.$uniform = unsafe { gl::GetUniformLocation(self.program.id, cptr!(concat!(stringify!($uniform), "\0"))) };
					if validate_uniforms {
						assert!(self.$uniform != gl::INVALID_VALUE as i32);
							assert!(self.$uniform != gl::INVALID_OPERATION as i32);
					}
				)*
			}

			#[cfg(feature = "live-shader-reload")]
			pub fn poll(&mut self) -> Result<bool, ShaderCreationError> {
					if self.program.poll()? {
							self.update(false);
							return Ok(true);
					}
					Ok(false)
			}
		}
	}
}

declare_program! { GridShaderProgram,
    SCREEN_SHADER_V_PATH, SCREEN_SHADER_V, SCREEN_SHADER_F_PATH, SCREEN_SHADER_F {
        u_screen_dim,
        u_cell_dim,
        u_atlas,
        u_color_bg,
        u_color_fg,
        u_glyph_ref,
        u_cursor,
        u_cursor_color,
        u_atlas_dim,
        u_main_pass
    }
}

impl GridShaderProgram {
    pub fn set_term_uniforms(&self, props: &SizeInfo) {
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

declare_program! { GlyphRectShaderProgram,
                GLYPHRECT_SHADER_V_PATH, GLYPHRECT_SHADER_V, GLYPHRECT_SHADER_F_PATH, GLYPHRECT_SHADER_F {
                u_atlas,
                u_scale
        }
}

fn create_shader(
    path: &str,
    kind: GLenum,
    source: Option<&'static str>,
) -> Result<GLuint, ShaderCreationError> {
    let from_disk;
    let source = if let Some(src) = source {
        src
    } else {
        from_disk = fs::read_to_string(path)?;
        &from_disk[..]
    };

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

        Err(ShaderCreationError::Compile(PathBuf::from(path), log))
    }
}

/// Rectangle drawing program.
///
/// Uniforms are prefixed with "u".
#[derive(Debug)]
pub struct RectShaderProgram {
    /// Program id.
    pub id: GLuint,
    /// Rectangle color.
    u_color: GLint,
}

impl RectShaderProgram {
    pub fn new() -> Result<Self, ShaderCreationError> {
        let (vertex_src, fragment_src) = if cfg!(feature = "live-shader-reload") {
            (None, None)
        } else {
            (Some(RECT_SHADER_V), Some(RECT_SHADER_F))
        };
        let vertex_shader = create_shader(RECT_SHADER_V_PATH, gl::VERTEX_SHADER, vertex_src)?;
        let fragment_shader = create_shader(RECT_SHADER_F_PATH, gl::FRAGMENT_SHADER, fragment_src)?;
        let program = create_program(vertex_shader, fragment_shader)?;

        unsafe {
            gl::DeleteShader(fragment_shader);
            gl::DeleteShader(vertex_shader);
            gl::UseProgram(program);
        }

        // Get uniform locations.
        let u_color = unsafe { gl::GetUniformLocation(program, b"color\0".as_ptr() as *const _) };

        let shader = Self { id: program, u_color };

        unsafe { gl::UseProgram(0) }

        Ok(shader)
    }
}

impl Drop for RectShaderProgram {
    fn drop(&mut self) {
        unsafe {
            gl::DeleteProgram(self.id);
        }
    }
}
