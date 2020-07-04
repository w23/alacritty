use crate::gl;
use crate::gl::types::*;
// use std::sync::mpsc;
use crate::renderer::{rects::RenderRect, Atlas, Error, GlyphCache, LoaderApi, ATLAS_SIZE};
use alacritty_terminal::{
    config::{self, Config, Delta, Font, StartupMode},
    index::{Column, Line},
    term::{self, color::Rgb, CursorKey, RenderableCell, RenderableCellContent, SizeInfo},
};

#[derive(Debug)]
pub struct SimpleRenderer {
    // program: TextShaderProgram,
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
}

#[derive(Debug)]
pub struct RenderApi<'a, C> {
    active_tex: &'a mut GLuint,
    // batch: &'a mut Batch,
    atlas: &'a mut Vec<Atlas>,
    current_atlas: &'a mut usize,
    // program: &'a mut TextShaderProgram,
    config: &'a Config<C>,
}

impl SimpleRenderer {
    pub fn new() -> Result<SimpleRenderer, Error> {
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
        };

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
        // Flush message queue.
        // if let Ok(Msg::ShaderReload) = self.rx.try_recv() {
        //     self.reload_shaders(props);
        // }
        // while self.rx.try_recv().is_ok() {}

        // unsafe {
        //     gl::UseProgram(self.program.id);
        //     self.program.set_term_uniforms(props);
        //
        //     gl::BindVertexArray(self.vao);
        //     gl::BindBuffer(gl::ELEMENT_ARRAY_BUFFER, self.ebo);
        //     gl::BindBuffer(gl::ARRAY_BUFFER, self.vbo_instance);
        //     gl::ActiveTexture(gl::TEXTURE0);
        // }

        let res = func(RenderApi {
            active_tex: &mut self.active_tex,
            // batch: &mut self.batch,
            atlas: &mut self.atlas,
            current_atlas: &mut self.current_atlas,
            // program: &mut self.program,
            config,
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
    //     let result = (TextShaderProgram::new(), RectShaderProgram::new());
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

    pub fn resize(&mut self, size: &SizeInfo) {
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
}

impl<'a, C> RenderApi<'a, C> {
    pub fn clear(&self, color: Rgb) {
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
        // let chars = match cell.inner {
        //     RenderableCellContent::Cursor(cursor_key) => {
        //         // Raw cell pixel buffers like cursors don't need to go through font lookup.
        //         let metrics = glyph_cache.metrics;
        //         let glyph = glyph_cache.cursor_cache.entry(cursor_key).or_insert_with(|| {
        //             self.load_glyph(&cursor::get_cursor_glyph(
        //                 cursor_key.style,
        //                 metrics,
        //                 self.config.font.offset.x,
        //                 self.config.font.offset.y,
        //                 cursor_key.is_wide,
        //                 self.config.cursor.thickness(),
        //             ))
        //         });
        //         self.add_render_item(cell, glyph);
        //         return;
        //     }
        //     RenderableCellContent::Chars(chars) => chars,
        // };
        //
        // // Get font key for cell.
        // let font_key = match cell.flags & Flags::BOLD_ITALIC {
        //     Flags::BOLD_ITALIC => glyph_cache.bold_italic_key,
        //     Flags::ITALIC => glyph_cache.italic_key,
        //     Flags::BOLD => glyph_cache.bold_key,
        //     _ => glyph_cache.font_key,
        // };
        //
        // // Don't render text of HIDDEN cells.
        // let mut chars = if cell.flags.contains(Flags::HIDDEN) {
        //     [' '; cell::MAX_ZEROWIDTH_CHARS + 1]
        // } else {
        //     chars
        // };
        //
        // // Render tabs as spaces in case the font doesn't support it.
        // if chars[0] == '\t' {
        //     chars[0] = ' ';
        // }
        //
        // let mut glyph_key = GlyphKey { font_key, size: glyph_cache.font_size, c: chars[0] };
        //
        // // Add cell to batch.
        // let glyph = glyph_cache.get(glyph_key, self);
        // //self.add_render_item(cell, glyph);
        //
        // self.update_main_texture_cell(cell, glyph);
        //
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
