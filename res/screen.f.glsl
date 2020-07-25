#version 330 core

//flat in vec4 bg;

layout(location = 0, index = 0) out vec4 color;
uniform sampler2D glyphRef;
uniform sampler2D atlas;
uniform vec2 cellDim;

void main() {
	vec2 uv = gl_FragCoord.xy;
	vec2 cell = floor(uv / cellDim);
	vec2 cell_uv = fract(uv / cellDim);
	//color = vec4(cell_uv, 0., 1.);

	vec4 glyph = texture(glyphRef, (cell + .5) / vec2(textureSize(glyphRef, 0)));
	//color = vec4(texture(atlas, uv / vec2(1000.)).rgb, 1);

	color = vec4(texture(atlas, glyph.xy + glyph.zw * cell_uv).rgb, 1.);

    // if (backgroundPass != 0) {
    //     if (bg.a == 0.0)
    //         discard;
    //
    //     alphaMask = vec4(1.0);
    //     color = vec4(bg.rgb, 1.0);
    // } else {
    //     if (colored != 0) {
    //         // Color glyphs, like emojis.
    //         vec4 glyphColor = texture(mask, TexCoords);
    //         alphaMask = vec4(glyphColor.a);
    //
    //         // Revert alpha premultiplication.
    //         if (glyphColor.a != 0) {
    //             glyphColor.rgb = vec3(glyphColor.rgb / glyphColor.a);
    //         }
    //
    //         color = vec4(glyphColor.rgb, 1.0);
    //     } else {
    //         // Regular text glyphs.
    //         vec3 textColor = texture(mask, TexCoords).rgb;
    //         alphaMask = vec4(textColor, textColor.r);
    //         color = vec4(fg, 1.0);
    //     }
    // }
}
