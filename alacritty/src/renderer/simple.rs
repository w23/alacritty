use crate::cursor;
use {
    crate::{
        config::ui_config::UIConfig,
        gl,
        gl::types::*,
        renderer::{rects::RenderRect, Error, Glyph, GlyphCache, LoadGlyph},
    },
    alacritty_terminal::{
        index::{Column, Line},
        term::{
            self,
            cell::{self, Flags, MAX_ZEROWIDTH_CHARS},
            color::Rgb,
            RenderableCell, RenderableCellContent, SizeInfo,
        },
    },
    log::*,
    std::{mem::size_of, ptr},
};

use crossfont::{GlyphKey, RasterizedGlyph};

use alacritty_terminal::config::Cursor;

use super::atlas::*;
use super::shade::*;
use super::texture::*;

#[derive(Debug, Clone)]
struct GlyphRef {
    x: u8,
    y: u8,
    z: u8,
    w: u8,
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
    vao: GLuint,
    vbo: GLuint,
    columns: usize,
    lines: usize,

    cursor_cell: [f32; 2],
    cursor_glyph: [f32; 2],
    cursor_color: Rgb,

    rect_program: RectShaderProgram,
    rect_vao: GLuint,
    rect_vbo: GLuint,
}

impl SimpleRenderer {
    pub fn new() -> Result<SimpleRenderer, Error> {
        let screen_glyphs_ref_tex = unsafe { create_texture(256, 256, PixelFormat::RGBA8) };
        let screen_colors_fg_tex = unsafe { create_texture(256, 256, PixelFormat::RGBA8) };
        let screen_colors_bg_tex = unsafe { create_texture(256, 256, PixelFormat::RGB8) };

        let mut vao: GLuint = 0;
        let mut vbo: GLuint = 0;

        let mut rect_vao: GLuint = 0;
        let mut rect_vbo: GLuint = 0;
        let mut rect_ebo: GLuint = 0;

        unsafe {
            gl::BlendFunc(gl::SRC_ALPHA, gl::ONE_MINUS_SRC_ALPHA);

            gl::DepthMask(gl::FALSE);

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

            // Rectangle setup.
            gl::GenVertexArrays(1, &mut rect_vao);
            gl::GenBuffers(1, &mut rect_vbo);
            gl::GenBuffers(1, &mut rect_ebo);
            gl::BindVertexArray(rect_vao);
            let indices: [i32; 6] = [0, 1, 3, 1, 2, 3];
            gl::BindBuffer(gl::ELEMENT_ARRAY_BUFFER, rect_ebo);
            gl::BufferData(
                gl::ELEMENT_ARRAY_BUFFER,
                (size_of::<i32>() * indices.len()) as _,
                indices.as_ptr() as *const _,
                gl::STATIC_DRAW,
            );

            // Cleanup.
            gl::BindVertexArray(0);
            gl::BindBuffer(gl::ARRAY_BUFFER, 0);
            gl::BindBuffer(gl::ELEMENT_ARRAY_BUFFER, 0);
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
            vao,
            vbo,
            columns: 0,
            lines: 0,

            cursor_cell: [-1.0; 2],
            cursor_glyph: [-1.0; 2],
            cursor_color: Rgb { r: 0, g: 0, b: 0 },

            rect_program: RectShaderProgram::new()?,
            rect_vao,
            rect_vbo,
        };

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

    /// Render a rectangle.
    ///
    /// This requires the rectangle program to be activated.
    fn render_rect(&mut self, rect: &RenderRect, size: &SizeInfo) {
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

        unsafe {
            // Setup vertices.
            let vertices: [f32; 8] = [x + width, y, x + width, y - height, x, y - height, x, y];

            // Load vertex data into array buffer.
            gl::BufferData(
                gl::ARRAY_BUFFER,
                (size_of::<f32>() * vertices.len()) as _,
                vertices.as_ptr() as *const _,
                gl::STATIC_DRAW,
            );

            // Color.
            self.rect_program.set_color(rect.color, rect.alpha);

            // Draw the rectangle.
            gl::DrawElements(gl::TRIANGLES, 6, gl::UNSIGNED_INT, ptr::null());
        }
    }

    pub fn begin<'a>(
        &'a mut self,
        config: &'a UIConfig,
        cursor_config: Cursor,
        size_info: &'a SizeInfo,
    ) -> RenderContext<'a> {
        RenderContext { this: self, size_info, config, cursor_config }
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

    fn paint(&mut self, size_info: &SizeInfo) {
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

        let atlas_dims = self.atlas.as_ref().unwrap().cell_dims();

        unsafe {
            gl::UseProgram(self.program.program.id);

            self.program.set_term_uniforms(size_info);
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

            gl::Uniform4f(
                self.program.u_atlas_dim,
                atlas_dims.off_x as f32,
                atlas_dims.off_y as f32,
                atlas_dims.size_x as f32,
                atlas_dims.size_y as f32,
            );

            gl::BindTexture(gl::TEXTURE_2D, self.atlas.as_ref().unwrap().tex);

            gl::ActiveTexture(gl::TEXTURE1);
            gl::BindTexture(gl::TEXTURE_2D, self.screen_glyphs_ref_tex);
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

            gl::BindVertexArray(self.vao);
            gl::BindBuffer(gl::ARRAY_BUFFER, self.vbo);
            gl::VertexAttribPointer(0, 2, gl::FLOAT, gl::FALSE, 0, ptr::null());
            gl::EnableVertexAttribArray(0);

            gl::DrawArrays(gl::TRIANGLE_STRIP, 0, 4);
            gl::DisableVertexAttribArray(0);
            gl::ActiveTexture(gl::TEXTURE0);
            gl::BindVertexArray(0);
        }
    }

    pub fn clear(&mut self, color: Rgb, background_opacity: f32) {
        self.screen_glyphs_ref.iter_mut().map(|x| *x = GlyphRef { x: 0, y: 0, z: 0, w: 0 }).count();
        self.screen_colors_fg.iter_mut().map(|x| *x = [0u8; 4]).count();
        self.screen_colors_bg.iter_mut().map(|x| *x = [color.r, color.g, color.b]).count();

        unsafe {
            let alpha = background_opacity;
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
}

impl LoadGlyph for SimpleRenderer {
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

    fn clear(&mut self) {
        self.atlas = None;
    }
}

#[derive(Debug)]
pub struct RenderContext<'a> {
    this: &'a mut SimpleRenderer,
    size_info: &'a term::SizeInfo,
    config: &'a UIConfig,
    cursor_config: Cursor,
}

impl<'a> RenderContext<'a> {
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
        trace!("render_string: {}", string);

        let bg_alpha = bg.map(|_| 1.0).unwrap_or(0.0);

        let cells = string
            .chars()
            .enumerate()
            .map(|(i, c)| RenderableCell {
                line,
                column: Column(i),
                inner: RenderableCellContent::Chars({
                    let mut chars = [' '; cell::MAX_ZEROWIDTH_CHARS + 1];
                    chars[0] = c;
                    chars
                }),
                flags: Flags::empty(),
                bg_alpha,
                fg,
                bg: bg.unwrap_or(Rgb { r: 0, g: 0, b: 0 }),
            })
            .collect::<Vec<_>>();

        for cell in cells {
            self.update_cell(cell, glyph_cache);
        }
    }

    pub fn update_cell(&mut self, cell: RenderableCell, glyph_cache: &mut GlyphCache) {
        if self.this.atlas.is_none() {
            self.this.atlas = Some(GridAtlas::new(self.size_info));
        }

        match cell.inner {
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
                self.this.set_cursor(
                    cell.column.0,
                    cell.line.0,
                    glyph.uv_left,
                    glyph.uv_bot,
                    cell.fg,
                );
            }

            // こんにちは
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

                let wide = match cell.flags & Flags::WIDE_CHAR {
                    Flags::WIDE_CHAR => true,
                    _ => false,
                };
                let glyph_key = GlyphKey { font_key, size: glyph_cache.font_size, c: chars[0] };
                let glyph = glyph_cache.get(glyph_key, self);

                let cell_index = cell.line.0 * self.this.columns + cell.column.0;

                trace!(
                    "{},{} -> {}: {:?}",
                    cell.line.0,
                    cell.column.0,
                    cell_index,
                    self.this.screen_glyphs_ref[cell_index]
                );

                // put glyph reference into texture data
                self.this.screen_glyphs_ref[cell_index] = GlyphRef {
                    x: glyph.uv_left as u8,
                    y: glyph.uv_bot as u8,
                    z: glyph.colored as u8,
                    w: 0,
                };

                self.this.screen_colors_fg[cell_index] =
                    [cell.fg.r, cell.fg.g, cell.fg.b, (cell.bg_alpha * 255.0) as u8];

                self.this.screen_colors_bg[cell_index] = [cell.bg.r, cell.bg.g, cell.bg.b];

                if wide && cell.column.0 < self.this.columns {
                    let cell_index = cell_index + 1;
                    self.this.screen_glyphs_ref[cell_index] = GlyphRef {
                        x: glyph.uv_left as u8 + 1,
                        y: glyph.uv_bot as u8,
                        z: glyph.colored as u8,
                        w: 0,
                    };
                    self.this.screen_colors_fg[cell_index] =
                        [cell.fg.r, cell.fg.g, cell.fg.b, (cell.bg_alpha * 255.0) as u8];
                    self.this.screen_colors_bg[cell_index] = [cell.bg.r, cell.bg.g, cell.bg.b];
                }

                // FIXME Render zero-width characters.
            }
        };
    }

    /// Draw all rectangles simultaneously to prevent excessive program swaps.
    pub fn draw_rects(&mut self, size_info: &term::SizeInfo, rects: Vec<RenderRect>) {
        // Swap to rectangle rendering program.
        unsafe {
            // Swap program.
            gl::UseProgram(self.this.rect_program.id);

            // Remove padding from viewport.
            gl::Viewport(0, 0, size_info.width as i32, size_info.height as i32);

            // Change blending strategy.
            gl::Enable(gl::BLEND);
            gl::BlendFuncSeparate(gl::SRC_ALPHA, gl::ONE_MINUS_SRC_ALPHA, gl::SRC_ALPHA, gl::ONE);

            // Setup data and buffers.
            gl::BindVertexArray(self.this.rect_vao);
            gl::BindBuffer(gl::ARRAY_BUFFER, self.this.rect_vbo);

            // Position.
            gl::VertexAttribPointer(
                0,
                2,
                gl::FLOAT,
                gl::FALSE,
                (size_of::<f32>() * 2) as _,
                ptr::null(),
            );
            gl::EnableVertexAttribArray(0);
        }

        // Draw all the rects.
        for rect in rects {
            // FIXME do in a single draw call
            self.this.render_rect(&rect, size_info);
        }

        // Deactivate rectangle program again.
        unsafe {
            // Reset blending strategy.
            gl::Disable(gl::BLEND);
            gl::BlendFunc(gl::SRC_COLOR, gl::ONE_MINUS_SRC_COLOR);

            // Reset data and buffers.
            gl::BindBuffer(gl::ARRAY_BUFFER, 0);
            gl::BindVertexArray(0);

            let padding_x = size_info.padding_x as i32;
            let padding_y = size_info.padding_y as i32;
            let width = size_info.width as i32;
            let height = size_info.height as i32;
            gl::Viewport(padding_x, padding_y, width - 2 * padding_x, height - 2 * padding_y);

            // Disable program.
            gl::UseProgram(0);
        }
    }

    pub fn draw_grid_text(&mut self) {
        self.this.paint(self.size_info);
    }
}

impl<'a> LoadGlyph for RenderContext<'a> {
    fn load_glyph(&mut self, rasterized: &RasterizedGlyph) -> Glyph {
        self.this.load_glyph(rasterized)
    }

    fn clear(&mut self) {
        LoadGlyph::clear(self.this);
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
        LoadGlyph::clear(self.renderer);
    }
}
