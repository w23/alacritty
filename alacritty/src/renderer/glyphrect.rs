use super::glyph::GeometryFree;
use super::math::*;
use super::shade::GlyphRectShaderProgram;
use crate::gl;
use crate::gl::types::*;
use crate::renderer::Error;
use alacritty_terminal::term::SizeInfo;
use log::*;

use std::mem::size_of;
use std::ptr;

pub enum RectAddError {
    Full,
}

pub struct GlyphRect {
    pub pos: Vec2<i16>,
    pub geom: GeometryFree,
    pub fg: alacritty_terminal::term::color::Rgb,
    pub colored: bool,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct Rgb {
    r: u8,
    g: u8,
    b: u8,
}

impl Rgb {
    fn from(color: alacritty_terminal::term::color::Rgb) -> Rgb {
        Rgb { r: color.r, g: color.g, b: color.b }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct Vertex {
    x: i16,
    y: i16,
    u: f32,
    v: f32,
    fg: Rgb,
    flags: u8,
}

#[derive(Debug)]
pub struct Rectifier {
    vao: GLuint,
    vbo: GLuint,
    ebo: GLuint,
    program: GlyphRectShaderProgram,
    indices: Vec<u16>,
    vertices: Vec<Vertex>,
    size_info: Option<SizeInfo>,
}

impl Rectifier {
    pub fn new() -> Result<Self, Error> {
        let mut vao: GLuint = 0;
        let mut vbo: GLuint = 0;
        let mut ebo: GLuint = 0;

        unsafe {
            gl::GenVertexArrays(1, &mut vao);
            gl::GenBuffers(1, &mut vbo);
            gl::GenBuffers(1, &mut ebo);
        }

        Ok(Self {
            vao,
            vbo,
            ebo,
            program: GlyphRectShaderProgram::new()?,
            indices: Vec::new(),
            vertices: Vec::new(),
            size_info: None,
        })
    }

    pub fn begin(&mut self, size_info: &SizeInfo) {
        self.indices.clear();
        self.vertices.clear();
        self.size_info = Some(*size_info);

        #[cfg(feature = "live-shader-reload")]
        {
            match self.program.poll() {
                Err(e) => {
                    error!("shader error: {}", e);
                }
                Ok(updated) if updated => {
                    debug!("updated shader: {:?}", self.program);
                }
                _ => {}
            }
        }
    }

    pub fn add(&mut self, glyph: &GlyphRect) -> Result<(), RectAddError> {
        let index = self.vertices.len();
        if index >= 65536 - 4 {
            return Err(RectAddError::Full);
        }
        let index = index as u16;

        let size_info = self.size_info.as_ref().unwrap();
        let g = glyph.geom;

        // Calculate rectangle position.
        let x = glyph.pos.x + g.left;
        let y = glyph.pos.y + (size_info.cell_height as i16 - g.top);
        let fg = Rgb::from(glyph.fg);
        let flags = if glyph.colored { 1 } else { 0 };

        self.vertices.push(Vertex {
            x,
            y: y + g.height,
            u: g.uv_left,
            v: g.uv_bot + g.uv_height,
            fg,
            flags,
        });
        self.vertices.push(Vertex { x, y, u: g.uv_left, v: g.uv_bot, fg, flags });
        self.vertices.push(Vertex {
            x: x + g.width,
            y: y + g.height,
            u: g.uv_left + g.uv_width,
            v: g.uv_bot + g.uv_height,
            fg,
            flags,
        });
        self.vertices.push(Vertex {
            x: x + g.width,
            y,
            u: g.uv_left + g.uv_width,
            v: g.uv_bot,
            fg,
            flags,
        });

        self.indices.push(index);
        self.indices.push(index + 1);
        self.indices.push(index + 2);

        self.indices.push(index + 2);
        self.indices.push(index + 3);
        self.indices.push(index + 1);

        Ok(())
    }

    pub fn draw(&self) {
        let size_info = self.size_info.as_ref().unwrap();
        if self.indices.is_empty() {
            return;
        }

        // Swap to rectangle rendering program.
        unsafe {
            // Add padding to viewport
            let pad_x = size_info.padding_x as i32;
            let pad_y = size_info.padding_y as i32;
            let width = size_info.width as i32 - 2 * pad_x;
            let height = size_info.height as i32 - 2 * pad_y;
            gl::Viewport(pad_x, pad_y, width, height);

            // Swap program.
            gl::UseProgram(self.program.program.id);

            // FIXME expect atlas to be bound at 0
            gl::Uniform1i(self.program.u_atlas, 0);
            gl::Uniform2f(self.program.u_scale, 2.0 / width as f32, -2.0 / height as f32);

            // Change blending strategy.
            gl::Enable(gl::BLEND);
            gl::BlendFuncSeparate(gl::SRC_ALPHA, gl::ONE_MINUS_SRC_ALPHA, gl::SRC_ALPHA, gl::ONE);

            // Setup data and buffers.
            gl::BindVertexArray(self.vao);

            gl::BindBuffer(gl::ELEMENT_ARRAY_BUFFER, self.ebo);
            gl::BufferData(
                gl::ELEMENT_ARRAY_BUFFER,
                (self.indices.len() * std::mem::size_of::<u16>()) as isize,
                self.indices.as_ptr() as *const _,
                gl::STREAM_DRAW,
            );

            gl::BindBuffer(gl::ARRAY_BUFFER, self.vbo);
            gl::BufferData(
                gl::ARRAY_BUFFER,
                (self.vertices.len() * std::mem::size_of::<Vertex>()) as isize,
                self.vertices.as_ptr() as *const _,
                gl::STREAM_DRAW,
            );

            // Position
            gl::VertexAttribPointer(
                0,
                2,
                gl::SHORT,
                gl::FALSE,
                (size_of::<Vertex>()) as _,
                ptr::null(),
            );
            gl::EnableVertexAttribArray(0);

            // uv
            gl::VertexAttribPointer(
                1,
                2,
                gl::FLOAT,
                gl::FALSE,
                (size_of::<Vertex>()) as _,
                offset_of!(Vertex, u) as *const _,
            );
            gl::EnableVertexAttribArray(1);

            // Foreground color
            gl::VertexAttribPointer(
                2,
                3,
                gl::UNSIGNED_BYTE,
                gl::TRUE,
                (size_of::<Vertex>()) as _,
                offset_of!(Vertex, fg) as *const _,
            );
            gl::EnableVertexAttribArray(2);

            // Flags
            gl::VertexAttribPointer(
                3,
                1,
                gl::UNSIGNED_BYTE,
                gl::FALSE,
                (size_of::<Vertex>()) as _,
                offset_of!(Vertex, flags) as *const _,
            );
            gl::EnableVertexAttribArray(3);

            gl::DrawElements(
                gl::TRIANGLES,
                self.indices.len() as i32,
                gl::UNSIGNED_SHORT,
                ptr::null(),
            );

            // Deactivate rectangle program again.
            // Reset blending strategy.
            gl::Disable(gl::BLEND);
            gl::BlendFunc(gl::SRC_COLOR, gl::ONE_MINUS_SRC_COLOR);

            // Reset data and buffers.
            gl::BindBuffer(gl::ARRAY_BUFFER, 0);
            gl::BindVertexArray(0);

            // FIXME ??? track viewport wrt padding properly everywhere
            let padding_x = size_info.padding_x as i32;
            let padding_y = size_info.padding_y as i32;
            let width = size_info.width as i32;
            let height = size_info.height as i32;
            gl::Viewport(padding_x, padding_y, width - 2 * padding_x, height - 2 * padding_y);

            // Disable program.
            gl::UseProgram(0);
        }
    }
}
