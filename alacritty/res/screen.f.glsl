#version 330 core

layout(location = 0, index = 0) out vec4 color;
uniform sampler2D glyph_ref;
uniform sampler2D atlas;
uniform sampler2D color_fg;
uniform sampler2D color_bg;
uniform vec4 screen_dim; // .xy = padding, .zw = resolution
uniform vec2 cell_dim;
uniform vec4 atlas_dim; // .xy = offset, .zw = cell_size
uniform vec4 cursor;
uniform vec3 cursor_color;

vec3 drawGlyph(vec4 glyph, vec2 cell_pix, vec3 bg, vec3 fg) {
	vec2 atlas_pix = glyph.xy * atlas_dim.zw + atlas_dim.xy + cell_pix;
	vec4 mask = texture(atlas, atlas_pix / textureSize(atlas, 0));

	//return mask.rgb;

	if (glyph.z > 0.)
		return mix(bg, mask.rgb, mask.a);
	else
		return mix(bg, fg.rgb, mask.rgb);
}

void main() {
	vec2 uv = gl_FragCoord.xy;
	uv.y = screen_dim.w - uv.y;

	uv.xy -= screen_dim.xy;

	vec2 cell = floor(uv / cell_dim);
	vec2 screen_cells = textureSize(glyph_ref, 0);

	if (any(lessThan(uv.xy, vec2(0.)))
			|| any(greaterThanEqual(cell, screen_cells))
	) {
		color = vec4(1., 0., 0., 1.);
		return;
	}

	vec2 cell_pix = mod(uv, cell_dim);

	vec2 tuv = (cell + .5) / vec2(textureSize(glyph_ref, 0));
	vec4 glyph = texture(glyph_ref, tuv);
	vec3 fg = texture(color_fg, tuv).rgb;
	vec3 bg = texture(color_bg, tuv).rgb;

	color = vec4(bg, 1.);

	if (cell == cursor.xy)
		color = vec4(drawGlyph(vec4(cursor.zw, 0., 0.), cell_pix, color.rgb, cursor_color), 1.);

	color = vec4(drawGlyph(vec4(glyph.xy * 255., glyph.zw), cell_pix, color.rgb, fg.rgb), 1.);

	//color = vec4(fg.rgb, 1.);
	//color = vec4(bg.rgb, 1.);
	//color = vec4(mask.rgb, 1.);
	//color = vec4(cursor_color, 1.);
	//color = mix(color, vec4(texture(atlas, uv / textureSize(atlas, 0)).rgb, 1), 1.);
}
