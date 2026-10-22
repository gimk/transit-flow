shader_type canvas_item;
// The open ground under everything (GridRenderer): grass in the node's colour, varied in
// world space so the map isn't one flat colour: broad patches of lusher and drier
// grass, smaller tufts, and a fine grain.

varying vec2 world;

void vertex() {
	world = (MODEL_MATRIX * vec4(VERTEX, 0.0, 1.0)).xy;
}

float hash(vec2 p) {
	return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453);
}

float value_noise(vec2 p) {
	vec2 i = floor(p);
	vec2 f = fract(p);
	vec2 u = f * f * (3.0 - 2.0 * f);
	return mix(mix(hash(i), hash(i + vec2(1.0, 0.0)), u.x),
		mix(hash(i + vec2(0.0, 1.0)), hash(i + vec2(1.0, 1.0)), u.x), u.y);
}

void fragment() {
	vec3 base = COLOR.rgb;
	float patches = value_noise(world / 420.0) * 0.65 + value_noise(world / 130.0) * 0.35;
	float tufts = value_noise(world / 18.0);
	float grain = value_noise(world / 3.0);
	vec3 lush = base * vec3(0.82, 1.02, 0.8);
	vec3 dry = base * vec3(1.22, 1.12, 0.82);
	vec3 color = mix(lush, dry, smoothstep(0.35, 0.7, patches));
	color *= 0.93 + 0.1 * tufts;
	color *= 0.97 + 0.06 * grain;
	COLOR = vec4(color, COLOR.a);
}
