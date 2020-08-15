#version 330 core

layout(location = 0, index = 0) out vec4 color;
uniform sampler2D glyphRef;
uniform sampler2D atlas;
uniform sampler2D color_fg;
uniform sampler2D color_bg;
uniform vec4 screenDim;
uniform vec2 cellDim;
uniform vec4 cursor;
uniform vec3 cursor_color;

const float atlas_grid_factor = 2.;

vec3 drawGlyph(vec4 glyph, vec2 cell_uv, vec3 bg, vec3 fg) {
	//vec2 atlas_pix = (glyph.xy + cell_uv) * cellDim * atlas_grid_factor;
	vec2 atlas_pix = (glyph.xy * atlas_grid_factor + .5 + cell_uv) * cellDim;
	vec4 mask = texture(atlas, (floor(atlas_pix) + .5) / textureSize(atlas, 0));

	//return mask.rgb;

	if (glyph.z > 0.)
		return mix(bg, mask.rgb, mask.a);
	else
		return mix(bg, fg.rgb, mask.rgb);
}

void main() {
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

	vec2 cell_uv = fract(uv / cellDim);

	vec2 tuv = (cell + .5) / vec2(textureSize(glyphRef, 0));
	vec4 glyph = texture(glyphRef, tuv);
	vec3 fg = texture(color_fg, tuv).rgb;
	vec3 bg = texture(color_bg, tuv).rgb;

	color = vec4(bg, 1.);

	if (cell == cursor.xy)
		color = vec4(drawGlyph(vec4(cursor.zw, 0., 0.), cell_uv, color.rgb, cursor_color), 1.);

	color = vec4(drawGlyph(vec4(glyph.xy * 255., glyph.zw), cell_uv, color.rgb, fg.rgb), 1.);

	//color = vec4(fg.rgb, 1.);
	//color = vec4(bg.rgb, 1.);
	//color = vec4(mask.rgb, 1.);
	//color = vec4(cursor_color, 1.);
	//color = mix(color, vec4(texture(atlas, uv / textureSize(atlas, 0)).rgb, 1), 1.);
}
