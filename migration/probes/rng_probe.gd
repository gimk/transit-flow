extends SceneTree
func _init() -> void:
	for s in [0, 7, 11, 23, 123456789]:
		var r := RandomNumberGenerator.new()
		r.seed = s
		var out := []
		for i in 4:
			out.append(str(r.randi()))
		for i in 4:
			out.append("%.9f" % r.randf())
		out.append("%.9f" % r.randf_range(2.0, 10.0))
		out.append(str(r.randi_range(0, 9)))
		print("seed ", s, ": ", " ".join(out))
	quit()
