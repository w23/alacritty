use crate::cursor;
use {
    crate::{
        config::ui_config::UIConfig,
        gl,
        gl::types::*,
        renderer::{rects::RenderRect, Error, GlyphCache, LoadGlyph},
    },
    alacritty_terminal::{
        index::{Column, Line},
        term::{
            self,
            cell::{self, Flags},
            color::Rgb,
            RenderableCell, RenderableCellContent, SizeInfo,
        },
    },
    log::*,
    std::ptr,
};

use alacritty_terminal::config::Cursor;

use super::atlas::{Atlas, AtlasInsertError, GridAtlas};
use super::glyph::{AtlasRef, Glyph, GlyphKey, RasterizedGlyph};
use super::glyphrect;
use super::math::*;
use super::shade::*;
use super::solidrect;
use super::texture::*;

#[derive(Debug, Clone)]
struct GlyphRef {
    x: u8,
    y: u8,
    z: u8,
    w: u8,
}

#[derive(Debug)]
struct Grid {
    atlas: GridAtlas,

    /// Screen worth of glyphs
    glyphs: Vec<GlyphRef>,
    // FIXME mark as empty on clear and don't paint if it is empty
}

impl Grid {
    fn new(
        index: usize,
        columns: usize,
        lines: usize,
        cell_size: Vec2<i32>,
        cell_offset: Vec2<i32>,
    ) -> Self {
        let cells = columns * lines;
        Self {
            atlas: GridAtlas::new(index, cell_size, cell_offset),
            glyphs: vec![GlyphRef { x: 0, y: 0, z: 0, w: 0 }; cells],
        }
    }

    fn resize(&mut self, columns: usize, lines: usize) {
        let cells = columns * lines;
        self.glyphs.resize(cells, GlyphRef { x: 0, y: 0, z: 0, w: 0 });
    }

    fn clear_atlas(&mut self, index: usize, cell_size: Vec2<i32>, cell_offset: Vec2<i32>) {
        self.atlas = GridAtlas::new(index, cell_size, cell_offset);
        self.glyphs.clear();
    }

    fn clear_grid(&mut self) {
        self.glyphs.iter_mut().for_each(|x| *x = GlyphRef { x: 0, y: 0, z: 0, w: 0 });
    }
}

#[derive(Debug)]
struct QuadGlyphBatches {
    atlas: Atlas,

    // FIXME each batch will have its own shader program. that's suboptimal
    batches: Vec<glyphrect::Rectifier>,
}

impl QuadGlyphBatches {
    fn new(index: usize) -> Self {
        Self { atlas: Atlas::new(index, 1024), batches: Vec::new() }
        // glyphrect::Rectifier::new().unwrap() }
    }

    fn clear_atlas(&mut self) {
        self.atlas.clear();
    }

    fn clear(&mut self) {
        for batch in &mut self.batches {
            batch.clear();
        }
    }

    fn add(&mut self, size_info: &SizeInfo, glyph_rect: &glyphrect::GlyphRect) {
        loop {
            if !self.batches.is_empty() {
                match self.batches.last_mut().unwrap().add(size_info, glyph_rect) {
                    Ok(_) => {
                        return;
                    }
                    Err(glyphrect::RectAddError::Full) => {}
                }
            }

            self.batches.push(glyphrect::Rectifier::new().unwrap());
        }
    }

    fn draw(&mut self, size_info: &SizeInfo) {
        unsafe {
            gl::ActiveTexture(gl::TEXTURE0);
            gl::BindTexture(gl::TEXTURE_2D, self.atlas.id);
        }

        for batch in &mut self.batches {
            // FIXME each of these draws will unnecessarily update lots of GL state that could be
            // shared
            batch.draw(size_info);
        }
    }
}

#[derive(Debug)]
pub struct SimpleRenderer {
    grids: Vec<Grid>,

    cell_size: Vec2<i32>,
    cell_offset: Vec2<i32>,

    screen_colors_fg: Vec<[u8; 4]>,
    screen_colors_bg: Vec<[u8; 3]>,

    // Texture that stores glyph->atlas references for the entire screen
    screen_glyphs_ref_tex: GLuint,
    screen_colors_fg_tex: GLuint,
    screen_colors_bg_tex: GLuint,
    background_opacity: f32,

    program: ScreenShaderProgram,
    vao: GLuint,
    vbo: GLuint,
    columns: usize,
    lines: usize,

    cursor_cell: [f32; 2],
    cursor_glyph: [f32; 2],
    cursor_color: Rgb,

    quad_glyph_batches: Vec<QuadGlyphBatches>,

    rectifier: solidrect::Rectifier,
}

impl SimpleRenderer {
    pub fn new() -> Result<SimpleRenderer, Error> {
        let screen_glyphs_ref_tex = unsafe { create_texture(256, 256, PixelFormat::RGBA8) };
        let screen_colors_fg_tex = unsafe { create_texture(256, 256, PixelFormat::RGBA8) };
        let screen_colors_bg_tex = unsafe { create_texture(256, 256, PixelFormat::RGB8) };

        let mut vao: GLuint = 0;
        let mut vbo: GLuint = 0;

        unsafe {
            //gl::BlendFunc(gl::SRC_ALPHA, gl::ONE_MINUS_SRC_ALPHA);
            gl::BlendFuncSeparate(gl::ONE, gl::ONE_MINUS_SRC_COLOR, gl::ONE, gl::ONE);

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

            // Cleanup.
            gl::BindVertexArray(0);
            gl::BindBuffer(gl::ARRAY_BUFFER, 0);
            gl::BindBuffer(gl::ELEMENT_ARRAY_BUFFER, 0);
        }

        Ok(Self {
            grids: Vec::new(),

            cell_size: Vec2 { x: 0, y: 0 },
            cell_offset: Vec2 { x: 0, y: 0 },

            screen_colors_fg: Vec::new(),
            screen_colors_bg: Vec::new(),
            background_opacity: 1.0,

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

            quad_glyph_batches: Vec::new(),

            rectifier: solidrect::Rectifier::new()?,
        })
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

    pub fn begin<'a>(
        &'a mut self,
        config: &'a UIConfig,
        cursor_config: Cursor,
        size_info: &'a SizeInfo,
    ) -> RenderContext<'a> {
        for batches in &mut self.quad_glyph_batches {
            batches.clear();
        }
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

        for grid in &mut self.grids {
            grid.resize(self.columns, self.lines);
        }
    }

    fn draw_grid_passes(&mut self, size_info: &SizeInfo) {
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

        unsafe {
            // Main pass blends glyphs on background manually in shader
            // and it needs to write the final color onto framebuffer as-is
            // so GL blending needs to be disabled
            gl::Disable(gl::BLEND);

            gl::UseProgram(self.program.program.id);

            gl::Uniform1f(self.program.u_background_opacity, self.background_opacity);

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
        }

        let mut main_pass = true;
        for grid in &self.grids {
            let atlas_dims = grid.atlas.cell_dims();
            unsafe {
                gl::Uniform4f(
                    self.program.u_atlas_dim,
                    atlas_dims.offset.x as f32,
                    // atlas_dims.offset.y as f32,
                    // Offset needs to be relative to "top" inverted-y OpenGL texture coords
                    (atlas_dims.size.y - atlas_dims.offset.y) as f32 - size_info.cell_height,
                    atlas_dims.size.x as f32,
                    atlas_dims.size.y as f32,
                );
                gl::Uniform1i(self.program.u_main_pass, main_pass as i32);

                gl::ActiveTexture(gl::TEXTURE0);
                gl::BindTexture(gl::TEXTURE_2D, grid.atlas.tex);

                gl::ActiveTexture(gl::TEXTURE1);
                gl::BindTexture(gl::TEXTURE_2D, self.screen_glyphs_ref_tex);
                upload_texture(
                    self.columns as i32,
                    self.lines as i32,
                    PixelFormat::RGBA8,
                    grid.glyphs.as_ptr() as *const _,
                );
                gl::DrawArrays(gl::TRIANGLE_STRIP, 0, 4);
            }

            if main_pass {
                unsafe {
                    // All further passes need to blend with framebuffer color
                    gl::Enable(gl::BLEND);
                    gl::BlendFuncSeparate(gl::ONE, gl::ONE_MINUS_SRC_COLOR, gl::ONE, gl::ONE);
                }
                main_pass = false;
            }
        }

        unsafe {
            gl::DisableVertexAttribArray(0);
            gl::ActiveTexture(gl::TEXTURE0);
            gl::BindVertexArray(0);
        }
    }

    pub fn clear(&mut self, color: Rgb, background_opacity: f32) {
        for grid in &mut self.grids {
            grid.clear_grid();
        }

        self.screen_colors_fg.iter_mut().for_each(|x| *x = [0u8; 4]);
        self.screen_colors_bg.iter_mut().for_each(|x| *x = [color.r, color.g, color.b]);
        self.background_opacity = background_opacity;

        unsafe {
            gl::ClearColor(0.0, 0.0, 0.0, 0.0);
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
        if !rasterized.wide && !rasterized.zero_width {
            if self.grids.is_empty() {
                self.grids.push(Grid::new(
                    0,
                    self.columns,
                    self.lines,
                    self.cell_size,
                    self.cell_offset,
                ));
            }
            loop {
                match self.grids.last_mut().unwrap().atlas.insert(rasterized) {
                    Ok(glyph) => {
                        return glyph;
                    }
                    Err(AtlasInsertError::GlyphTooLarge) => {
                        trace!(
                            "Glyph is too large for grid atlas, will render it using quads: {:?}",
                            rasterized
                        );
                        break;
                    }
                    Err(AtlasInsertError::Full) => {
                        debug!("GridAtlas is full, creating a new one");
                        let index = self.grids.len();
                        self.grids.push(Grid::new(
                            index,
                            self.columns,
                            self.lines,
                            self.cell_size,
                            self.cell_offset,
                        ));
                    }
                }
            }
        }

        for batches in &mut self.quad_glyph_batches {
            match batches.atlas.insert(rasterized) {
                Ok(glyph) => {
                    return glyph;
                }
                Err(AtlasInsertError::GlyphTooLarge) => {
                    panic!("FIXME handle this by returning dummy 0 glyph");
                }
                Err(AtlasInsertError::Full) => {}
            }
        }

        self.quad_glyph_batches.push(QuadGlyphBatches::new(self.quad_glyph_batches.len()));
        match self.quad_glyph_batches.last_mut().unwrap().atlas.insert(rasterized) {
            Ok(glyph) => glyph,
            Err(AtlasInsertError::GlyphTooLarge) => {
                panic!("FIXME handle this by returning dummy 0 glyph");
            }
            Err(AtlasInsertError::Full) => {
                panic!("New atlas is already full?!");
            }
        }
    }

    fn clear(&mut self, cell_size: Vec2<i32>, cell_offset: Vec2<i32>) {
        self.cell_size = cell_size;
        self.cell_offset = cell_offset;

        for quad in &mut self.quad_glyph_batches {
            quad.clear_atlas();
        }

        for (index, grid) in &mut self.grids.iter_mut().enumerate() {
            grid.clear_atlas(index, cell_size, cell_offset);
        }
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
        let wide = match cell.flags & Flags::WIDE_CHAR {
            Flags::WIDE_CHAR => true,
            _ => false,
        };

        match cell.inner {
            RenderableCellContent::Cursor(cursor_key) => {
                // Raw cell pixel buffers like cursors don't need to go through font lookup.
                let metrics = glyph_cache.metrics;
                let glyph = glyph_cache.cursor_cache.entry(cursor_key).or_insert_with(|| {
                    self.load_glyph(&RasterizedGlyph {
                        wide,
                        zero_width: false,
                        rasterized: cursor::get_cursor_glyph(
                            cursor_key.style,
                            metrics,
                            self.config.font.offset.x,
                            self.config.font.offset.y,
                            cursor_key.is_wide,
                            self.cursor_config.thickness(),
                        ),
                    })
                });

                match glyph.atlas_ref {
                    AtlasRef::Grid(grid) => {
                        self.this.set_cursor(
                            cell.column.0,
                            cell.line.0,
                            grid.column as f32,
                            grid.line as f32,
                            cell.fg,
                        );
                    }

                    // FIXME how to draw this cursor
                    _ => {
                        trace!("FIXME Non-grid cursor is broken");
                    }
                }
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

                let cell_index = cell.line.0 * self.this.columns + cell.column.0;
                self.this.screen_colors_fg[cell_index] =
                    [cell.fg.r, cell.fg.g, cell.fg.b, (cell.bg_alpha * 255.0) as u8];
                self.this.screen_colors_bg[cell_index] = [cell.bg.r, cell.bg.g, cell.bg.b];

                if wide && cell.column.0 < self.this.columns {
                    self.this.screen_colors_bg[cell_index + 1] = [cell.bg.r, cell.bg.g, cell.bg.b];
                }

                self.push_char(
                    GlyphKey {
                        wide,
                        zero_width: false,
                        key: crossfont::GlyphKey {
                            font_key,
                            size: glyph_cache.font_size,
                            c: chars[0],
                        },
                    },
                    &cell,
                    glyph_cache,
                    cell_index,
                    false,
                );

                // if wide && cell.column.0 < self.this.columns {
                //     let cell_index = cell_index + 1;
                //     self.this.screen_glyphs_ref[cell_index] = GlyphRef {
                //         x: glyph.uv_left as u8 + 1,
                //         y: glyph.uv_bot as u8,
                //         z: glyph.colored as u8,
                //         w: 0,
                //     };
                //     self.this.screen_colors_fg[cell_index] =
                //         [cell.fg.r, cell.fg.g, cell.fg.b, (cell.bg_alpha * 255.0) as u8];
                //     self.this.screen_colors_bg[cell_index] = [cell.bg.r, cell.bg.g, cell.bg.b];
                // }

                // Render zero-width characters.
                for c in (&chars[1..]).iter().filter(|c| **c != ' ') {
                    self.push_char(
                        GlyphKey {
                            wide,
                            zero_width: true,
                            key: crossfont::GlyphKey {
                                font_key,
                                size: glyph_cache.font_size,
                                c: *c,
                            },
                        },
                        &cell,
                        glyph_cache,
                        cell_index,
                        true,
                    );
                }
            }
        };
    }

    fn push_char(
        &mut self,
        glyph_key: GlyphKey,
        cell: &RenderableCell,
        glyph_cache: &mut GlyphCache,
        cell_index: usize,
        zero_width: bool,
    ) {
        let glyph = glyph_cache.get(glyph_key, self);

        match glyph.atlas_ref {
            AtlasRef::Grid(grid) => {
                // trace!(
                //     "{},{} -> {}: {:?}",
                //     cell.line.0,
                //     cell.column.0,
                //     cell_index,
                //     self.this.screen_glyphs_ref[cell_index]
                // );

                // put glyph reference into texture data
                self.this.grids[glyph.atlas_index as usize].glyphs[cell_index] = GlyphRef {
                    x: grid.column as u8,
                    y: grid.line as u8,
                    z: glyph.colored as u8,
                    w: 0,
                };
            }
            AtlasRef::Free(free) => {
                let glyph_rect = glyphrect::GlyphRect {
                    pos: Vec2::<i16> {
                        x: (if zero_width {
                            // The metrics of zero-width characters are based on rendering
                            // the character after the current cell, with the anchor at the
                            // right side of the preceding character. Since we render the
                            // zero-width characters inside the preceding character, the
                            // anchor has been moved to the right by one cell.
                            // FIXME WHY????
                            1
                        } else {
                            0
                        } + cell.column.0 as i16)
                            * self.size_info.cell_width as i16,
                        y: cell.line.0 as i16 * self.size_info.cell_height as i16,
                    },
                    geom: free,
                    fg: cell.fg,
                    colored: glyph.colored,
                };

                self.this.quad_glyph_batches[glyph.atlas_index].add(self.size_info, &glyph_rect);
            }
        }
    }

    /// Draw all rectangles simultaneously to prevent excessive program swaps.
    pub fn draw_rects(&mut self, size_info: &term::SizeInfo, rects: Vec<RenderRect>) {
        self.this.rectifier.begin(size_info);
        // Draw all the rects.
        for rect in rects {
            self.this.rectifier.add(&rect);
        }
        self.this.rectifier.draw();
    }

    pub fn draw_grid_text(&mut self) {
        debug!(
            "Grids: {}, quad atlas-batches: {}",
            self.this.grids.len(),
            self.this.quad_glyph_batches.len()
        );
        self.this.draw_grid_passes(self.size_info);
        for batches in &mut self.this.quad_glyph_batches {
            batches.draw(self.size_info);
        }
    }
}

impl<'a> LoadGlyph for RenderContext<'a> {
    fn load_glyph(&mut self, rasterized: &RasterizedGlyph) -> Glyph {
        self.this.load_glyph(rasterized)
    }

    fn clear(&mut self, cell_size: Vec2<i32>, cell_offset: Vec2<i32>) {
        LoadGlyph::clear(self.this, cell_size, cell_offset);
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

    fn clear(&mut self, cell_size: Vec2<i32>, cell_offset: Vec2<i32>) {
        LoadGlyph::clear(self.renderer, cell_size, cell_offset);
    }
}
