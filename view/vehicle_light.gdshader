shader_type canvas_item;
render_mode blend_add;
// The light vehicles throw on the road (VehicleRenderer's light pools): headlight beams
// at night, a red glow behind braking vehicles, an orange flash beside a blinking
// indicator. Drawn from the same instances as vehicle.gdshader but behind every body,
// so the beam of one car lights the road, never the roof of the car in front. Divided by
// the world's tint (DayNight) so the light keeps its own colour and strength at night.
// UV and INSTANCE_CUSTOM as in vehicle.gdshader.

uniform float night = 0.0;
uniform vec3 tint = vec3(1.0);

const float BEAM_LENGTH = 1.6; // In vehicle lengths, past the front bumper.
const vec3 HEADLAMP = vec3(1.0, 0.94, 0.78);
const vec3 BRAKE_LAMP = vec3(1.0, 0.12, 0.08);
const vec3 INDICATOR = vec3(1.0, 0.55, 0.05);

varying flat vec4 custom;

void vertex() {
	custom = INSTANCE_CUSTOM;
}

// A soft round pool of `radius` px around `center`.
float pool(vec2 q, vec2 center, float radius) {
	return pow(max(1.0 - length(q - center) / radius, 0.0), 1.5);
}

void fragment() {
	int flags = int(custom.y + 0.5);
	vec2 size = custom.zw;
	float hx = size.x * 0.5;
	float hy = size.y * 0.5;
	vec2 q = UV * size;

	float ahead = q.x - hx;
	float beam = 0.0;
	if (ahead > 0.0 && night > 0.0) {
		float reach = BEAM_LENGTH * size.x;
		float spread = hy * 0.7 + ahead * 0.35;
		beam = night * 0.55 * max(1.0 - ahead / reach, 0.0) * (1.0 - smoothstep(spread * 0.6, spread, abs(q.y)));
	}
	vec3 light = HEADLAMP * beam;
	if ((flags & 1) != 0) {
		light += BRAKE_LAMP * (0.3 + 0.35 * night) * pool(q, vec2(-hx, 0.0), hy * 1.6);
	}
	if (fract(TIME * 1.5) < 0.5) {
		float side = (flags & 2) != 0 ? -1.0 : (flags & 4) != 0 ? 1.0 : 0.0;
		if (side != 0.0) {
			float flash = pool(q, vec2(hx, side * hy), hy * 1.2) + pool(q, vec2(-hx, side * hy), hy * 1.2);
			light += INDICATOR * (0.25 + 0.35 * night) * flash;
		}
	}
	// The lamps themselves, as sources: under the body in the old look (which draws its
	// own lamps), the only light allowed over a vehicle under the dot display.
	float lamp_y = max(hy - 1.6, 0.5);
	float heads = night * (pool(q, vec2(hx, -lamp_y), 1.6) + pool(q, vec2(hx, lamp_y), 1.6));
	float tails = ((flags & 1) != 0 ? 1.0 : 0.5 * night) * (pool(q, vec2(-hx, -lamp_y), 1.6) + pool(q, vec2(-hx, lamp_y), 1.6));
	light += HEADLAMP * heads * 1.5 + BRAKE_LAMP * tails * 1.5;
	if (fract(TIME * 1.5) < 0.5 && (flags & 6) != 0) {
		float side = (flags & 2) != 0 ? -hy : hy;
		light += INDICATOR * 1.5 * (pool(q, vec2(hx, side), 1.6) + pool(q, vec2(-hx, side), 1.6));
	}
	COLOR = vec4(light / max(tint, vec3(0.05)), 1.0);
}
