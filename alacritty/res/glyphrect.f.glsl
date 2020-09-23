#version 300 es
precision mediump float;

uniform sampler2D atlas;

smooth in vec2 uv;
flat in vec3 fg;
flat in float flags;

out vec4 FragColor;

void main() {
		FragColor = vec4(0.);
		vec4 mask = texture(atlas, uv);
		bool colored = flags > 0.;
		if (colored) {
			if (mask.a > 0.) {
				mask.rgb /= mask.a;
				FragColor = mask;
			}
		} else {
			FragColor = vec4(fg, mask.r);
		}
}
