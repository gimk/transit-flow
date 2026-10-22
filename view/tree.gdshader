shader_type canvas_item;
// A tree seen from above (NatureRenderer): a lumpy round canopy in the instance colour,
// lit from the north-west, over its shadow cast down and right. UV runs -0.5..0.5 across
// the quad, which is NatureRenderer.QUAD_SCALE canopy radii wide.

const float QUAD_SCALE = 3.2;
const vec2 SHADOW_OFFSET = vec2(0.35, 0.45);
const vec2 SUN = vec2(-0.6, -0.8);

void fragment() {
	vec2 q = UV * QUAD_SCALE; // In canopy radii.
	float aa = max(fwidth(q.x), 0.001);
	// Lumps round the edge, different per tree (phase from its colour).
	float phase = fract(COLOR.r * 97.0 + COLOR.g * 41.0) * 6.2831;
	float angle = atan(q.y, q.x);
	float edge = 1.0 + 0.07 * sin(angle * 7.0 + phase) + 0.04 * sin(angle * 3.0 - phase);
	float canopy = 1.0 - smoothstep(edge - aa, edge + aa, length(q));
	float shadow = (1.0 - smoothstep(1.0 - aa, 1.0 + aa, length(q - SHADOW_OFFSET))) * 0.32;
	// Brighter toward the light, darker at the rim.
	float lit = dot(q, SUN) * 0.5 + 0.5;
	vec3 color = COLOR.rgb * (0.78 + 0.35 * lit);
	color = mix(color, COLOR.rgb * 0.6, smoothstep(0.7, 1.0, length(q) / edge) * 0.6);
	float highlight = 1.0 - smoothstep(0.0, 0.35, length(q - vec2(-0.35, -0.4)));
	color = mix(color, COLOR.rgb * 1.3, highlight * 0.5);
	vec4 tree = vec4(color, canopy);
	vec4 under = vec4(0.0, 0.0, 0.0, shadow);
	COLOR = mix(under, tree, canopy);
	COLOR.a = max(canopy, shadow);
}
