use std::mem::size_of;
use std::ptr;

// use log::*;

use alacritty_terminal::term::{RenderableCell, SizeInfo};

use crate::gl;
use crate::gl::types::*;
use crate::renderer::{Atlas, Error, Glyph, TextShaderProgram};

const BATCH_MAX: usize = 0x1_0000;

#[derive(Debug)]
pub struct Batcher {
    // Group accumulated batches by their atlases
    atlas_batches: Vec<AtlasBatch>,

    // GL objects for shared use. There's no point in having these per atlas/batch, as their
    // content is completely transient currently.
    vao: GLuint,
    vbo: GLuint,
    ebo: GLuint,
}

impl Batcher {
    pub fn new() -> Self {
        let mut vao: GLuint = 0;
        let mut vbo: GLuint = 0;
        let mut ebo: GLuint = 0;

        // Pre-generate indices once.
        // TODO there should be a solution using flat_map, but I failed to find one.
        let indices = {
            let mut indices = Vec::<u16>::new();
            for index in 0 as u16..(BATCH_MAX / 4) as u16 {
                let i = index * 4;
                indices.push(i);
                indices.push(i + 1);
                indices.push(i + 2);

                indices.push(i + 2);
                indices.push(i + 3);
                indices.push(i + 1);
            }
            indices
        };

        unsafe {
            gl::GenVertexArrays(1, &mut vao);
            gl::GenBuffers(1, &mut vbo);
            gl::GenBuffers(1, &mut ebo);

            // Create VAO and set up bindings just once.
            gl::BindVertexArray(vao);
            gl::BindBuffer(gl::ARRAY_BUFFER, vbo);

            // Position.
            gl::VertexAttribPointer(
                0,
                2,
                gl::SHORT,
                gl::FALSE,
                (size_of::<Vertex>()) as _,
                ptr::null(),
            );
            gl::EnableVertexAttribArray(0);

            // uv.
            gl::VertexAttribPointer(
                1,
                2,
                gl::FLOAT,
                gl::FALSE,
                (size_of::<Vertex>()) as _,
                offset_of!(Vertex, u) as *const _,
            );
            gl::EnableVertexAttribArray(1);

            // Foreground color.
            gl::VertexAttribPointer(
                2,
                3,
                gl::UNSIGNED_BYTE,
                gl::TRUE,
                (size_of::<Vertex>()) as _,
                offset_of!(Vertex, fg) as *const _,
            );
            gl::EnableVertexAttribArray(2);

            // Flags.
            gl::VertexAttribPointer(
                3,
                1,
                gl::UNSIGNED_BYTE,
                gl::FALSE,
                (size_of::<Vertex>()) as _,
                offset_of!(Vertex, flags) as *const _,
            );
            gl::EnableVertexAttribArray(3);

            // Pre-upload indices.
            gl::BindBuffer(gl::ELEMENT_ARRAY_BUFFER, ebo);
            gl::BufferData(
                gl::ELEMENT_ARRAY_BUFFER,
                (indices.len() * std::mem::size_of::<u16>()) as isize,
                indices.as_ptr() as *const _,
                gl::STATIC_DRAW,
            );
        }
        Self { vao, vbo, ebo, atlas_batches: Vec::new() }
    }

    pub fn bind(&self, size_info: &SizeInfo, program: &TextShaderProgram) {
        unsafe {
            // // Add padding to viewport.
            let pad_x = size_info.padding_x() as i32;
            let pad_y = size_info.padding_y() as i32;
            let width = size_info.width() as i32 - 2 * pad_x;
            let height = size_info.height() as i32 - 2 * pad_y;
            // gl::Viewport(pad_x, pad_y, width, height);

            // // Change blending strategy.
            // gl::Enable(gl::BLEND);
            // gl::BlendFuncSeparate(gl::SRC_ALPHA, gl::ONE_MINUS_SRC_ALPHA, gl::SRC_ALPHA,
            // gl::ONE);

            // Swap program.
            gl::UseProgram(program.id);

            // gl::Uniform1i(program.u_atlas, 0);
            gl::Uniform2f(program.u_scale, 2.0 / width as f32, -2.0 / height as f32);

            // Set VAO bindings.
            gl::BindVertexArray(self.vao);

            // VBO is not part of VAO state. VBO binding will be used for uploading vertex data.
            gl::BindBuffer(gl::ARRAY_BUFFER, self.vbo);
        }
    }

    // pub fn add_to_render(&mut self, size_info: &SizeInfo, glyph: &GlyphQuad<'_>) {
    pub fn add_item(&mut self, atlas_index: usize, cell: &RenderableCell, glyph: &Glyph) {
        if glyph.atlas_index >= self.atlas_batches.len() {
            self.atlas_batches.resize_with(glyph.atlas_index, Default::default);
        }

        self.atlas_batches[glyph.atlas_index].add(cell, glyph);
    }

    pub fn draw(
        &mut self,
        atlases: &Vec<Atlas>,
        size_info: &SizeInfo,
        program: &TextShaderProgram,
    ) {
        for (index, group) in &mut self.atlas_batches.iter().enumerate() {
            group.draw(atlases[index].id);
        }
    }
}

enum RectAddError {
    Full,
}

#[derive(Debug, Default)]
struct AtlasBatch {
    // A single atlas can have > BATCH_MAX vertices to accumulate, so we need to anticipate
    // multiple batches per atlas
    batches: Vec<Batch>,
}

impl AtlasBatch {
    fn new() -> Self {
        Self { batches: Vec::new() }
    }

    fn clear(&mut self) {
        for batch in &mut self.batches {
            batch.clear();
        }
    }

    fn add(&mut self, size_info: &SizeInfo, cell: &RenderableCell, glyph: &Glyph) {
        loop {
            if !self.batches.is_empty() {
                match self.batches.last_mut().unwrap().add(size_info, cell, glyph) {
                    Ok(_) => {
                        return;
                    },
                    Err(RectAddError::Full) => {},
                }
            }

            self.batches.push(Batch::new().unwrap());
        }
    }

    fn draw(&mut self, tex_id: GLuint) {
        // Binding to active slot 0
        unsafe {
            gl::BindTexture(gl::TEXTURE_2D, tex_id);
        }

        for batch in &mut self.batches {
            batch.draw();
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct Rgb {
    r: u8,
    g: u8,
    b: u8,
}

// #[repr(C)]
// #[derive(Debug, Clone, Copy)]
// struct Rgba {
//     r: u8,
//     g: u8,
//     b: u8,
//     a: u8,
// }

impl Rgb {
    fn from(color: alacritty_terminal::term::color::Rgb) -> Rgb {
        Rgb { r: color.r, g: color.g, b: color.b }
    }
}

// impl Rgba {
//     fn from(color: alacritty_terminal::term::color::Rgb, alpha: f32) -> Rgba {
//         Rgba { r: color.r, g: color.g, b: color.b, a: (alpha * 255.0) as u8 }
//     }
// }

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct Vertex {
    x: i16,
    y: i16,
    // TODO these can also be u/i16
    u: f32,
    v: f32,
    fg: Rgb,
    // bg: Rgba,
    flags: u8,
}

#[derive(Debug)]
struct Batch {
    vertices: Vec<Vertex>,
}

impl Batch {
    fn new() -> Result<Self, Error> {
        Ok(Self { vertices: Vec::new() })
    }

    fn clear(&mut self) {
        self.vertices.clear();
    }

    fn add(
        &mut self,
        size_info: &SizeInfo,
        cell: &RenderableCell,
        glyph: &Glyph,
    ) -> Result<(), RectAddError> {
        let index = self.vertices.len();
        if index >= BATCH_MAX - 4 {
            return Err(RectAddError::Full);
        }

        // Calculate rectangle position.
        let x = cell.column.0 as i16 * size_info.cell_width() as i16 + glyph.left;
        let y = (cell.line.0 + 1) as i16 * size_info.cell_height() as i16 - glyph.top;
        let fg = Rgb::from(cell.fg);
        // let bg = Rgba::from(cell.bg, cell.bg_alpha);
        let flags = glyph.multicolor;

        self.vertices.push(Vertex {
            x,
            y: y + glyph.height,
            u: glyph.uv_left,
            v: glyph.uv_bot + glyph.uv_height,
            fg,
            // bg,
            flags,
        });
        self.vertices.push(Vertex {
            x,
            y,
            u: glyph.uv_left,
            v: glyph.uv_bot,
            fg,
            // bg,
            flags,
        });
        self.vertices.push(Vertex {
            x: x + glyph.width,
            y: y + glyph.height,
            u: glyph.uv_left + glyph.uv_width,
            v: glyph.uv_bot + glyph.uv_height,
            fg,
            // bg,
            flags,
        });
        self.vertices.push(Vertex {
            x: x + glyph.width,
            y,
            u: glyph.uv_left + glyph.uv_width,
            v: glyph.uv_bot,
            fg,
            // bg,
            flags,
        });

        Ok(())
    }

    fn draw(&mut self) {
        if self.vertices.is_empty() {
            return;
        }

        unsafe {
            gl::BufferData(
                gl::ARRAY_BUFFER,
                (self.vertices.len() * std::mem::size_of::<Vertex>()) as isize,
                self.vertices.as_ptr() as *const _,
                gl::STREAM_DRAW,
            );

            // self.program.set_background_pass(true);
            // gl::DrawElements(
            //     gl::TRIANGLES,
            //     (self.vertices.len() / 4 * 6) as i32,
            //     gl::UNSIGNED_SHORT,
            //     ptr::null(),
            // );
            //
            // self.program.set_background_pass(false);
            gl::DrawElements(
                gl::TRIANGLES,
                (self.vertices.len() / 4 * 6) as i32,
                gl::UNSIGNED_SHORT,
                ptr::null(),
            );
        }
    }
}
