use crate::gl;
use crate::gl::types::*;

// use std::ptr;

pub enum PixelFormat {
    RGBA8,
    RGB8,
}

struct TextureFormat {
    internal: i32,
    format: u32,
    texel_type: u32,
    pixel_bytes: u32,
}

fn get_gl_format(format: PixelFormat) -> TextureFormat {
    match format {
        PixelFormat::RGBA8 => TextureFormat {
            internal: gl::RGBA as i32,
            format: gl::RGBA,
            texel_type: gl::UNSIGNED_BYTE,
            pixel_bytes: 4,
        },
        PixelFormat::RGB8 => TextureFormat {
            internal: gl::RGB as i32,
            format: gl::RGB,
            texel_type: gl::UNSIGNED_BYTE,
            pixel_bytes: 3,
        },
    }
}

pub unsafe fn upload_texture(
    width: i32,
    height: i32,
    format: PixelFormat,
    ptr: *const libc::c_void,
) {
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
        ptr,
    );
}

pub unsafe fn create_texture(width: i32, height: i32, format: PixelFormat) -> GLuint {
    let mut id: GLuint = 0;
    let format = get_gl_format(format);

    gl::PixelStorei(gl::UNPACK_ALIGNMENT, 1);

    gl::GenTextures(1, &mut id);
    gl::BindTexture(gl::TEXTURE_2D, id);

    // Why, OpenGL? Just why.
    let zero = vec![0u8; width as usize * height as usize * format.pixel_bytes as usize];

    gl::TexImage2D(
        gl::TEXTURE_2D,
        0,
        format.internal,
        width,
        height,
        0,
        format.format,
        format.texel_type,
        zero.as_ptr() as *const _,
    );

    gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::CLAMP_TO_EDGE as i32);
    gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::CLAMP_TO_EDGE as i32);
    gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::NEAREST as i32);
    gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::NEAREST as i32);

    gl::BindTexture(gl::TEXTURE_2D, 0);
    id
}
