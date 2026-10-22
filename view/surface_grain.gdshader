shader_type canvas_item;
// Breaks up flat paving colours (asphalt, sidewalks) with soft noise in world space, so
// it runs on seamlessly from one road piece to the next and doesn't swim with the camera.

uniform float scale = 0.35;
uniform float strength = 0.1;

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
	float n = value_noise(world * scale) * 0.6 + value_noise(world * scale * 3.7) * 0.4;
	COLOR.rgb *= 1.0 + strength * (n - 0.5) * 2.0;
}
