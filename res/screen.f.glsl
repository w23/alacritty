#version 330 core

//flat in vec4 bg;

layout(location = 0, index = 0) out vec4 color;
uniform sampler2D glyphRef;
uniform sampler2D atlas;
uniform sampler2D color_fg;
uniform sampler2D color_bg;
uniform vec4 screenDim;
uniform vec2 cellDim;

void main() {
	//color = vec4(texture(glyphRef, gl_FragCoord.xy / screenDim.zw).rgb, 1.);
	vec2 uv = gl_FragCoord.xy;
	uv.y = screenDim.w - uv.y;

	uv.xy -= screenDim.xy;

	vec2 cell = floor(uv / cellDim);
	vec2 screen_cells = textureSize(glyphRef, 0);

	if (any(lessThan(uv.xy, vec2(0.)))
			|| any(greaterThanEqual(cell, screen_cells))
	) {
		color = vec4(1., 0., 0., 1.);
		return;
	}

	//vec2 cell = floor(uv / cellDim);
	vec2 cell_uv = fract(uv / cellDim);
	//color = vec4(cell_uv, 0., 1.);
	//color = vec4(cell / 16., 0., 1.);

	vec2 tuv = (cell + .5) / vec2(textureSize(glyphRef, 0));
	vec4 glyph = texture(glyphRef, tuv);
	vec4 fg = texture(color_fg, tuv);
	vec3 bg = texture(color_bg, tuv).rgb;
	//color = vec4(fract(glyph.xy), 0., 1.);
	//color = vec4(0., 1., 0., 1.);
	//color = vec4(texture(atlas, uv / vec2(1000.)).rgb, 1);
	//color = vec4(texture(atlas, uv / vec2(1024.)).bbb, 1.);

	//glyph.x = 1. - glyph.x;
	//glyph.y = 1. - glyph.y;
	//cell_uv.x = 1. - cell_uv.x;

	vec4 mask = texture(atlas, glyph.xy + glyph.zw * cell_uv);
	//color = vec4(mask.rgb, 1.);
	color = vec4(mix(bg, fg.rgb, mask.rgb), 1.);

	//color = vec4(texture(atlas, glyph.xy + glyph.zw * cell_uv).rgb, 1.);
	//color = vec4(1., 0., 0., 1.);

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
