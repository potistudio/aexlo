// Drives the aexlo probe effect inside real After Effects.
//
// `playground ae` prepends one line, `var AEXLO = {...};`, with the run
// configuration (input image, effect match name, parameter values, frames to
// render, project depths for the smart variant, result path) and hands the
// script to AfterFX.exe with `-r`.
//
// The script only adds items to the open project and removes them again: it
// never creates, closes or saves a project unless `AEXLO.quit` is set, which
// the harness sets only for an After Effects instance it started itself.
//
// ExtendScript is ES3: no JSON object, no Array.prototype.map, no let/const.

(function () {
	var cfg = AEXLO;
	var result = { ok: false, ae_version: app.version, renders: [], params: [] };
	var folder = null;
	var originalDepth = null;

	function quote(text) {
		return '"' + String(text)
			.replace(/\\/g, "\\\\")
			.replace(/"/g, '\\"')
			.replace(/\r/g, "\\r")
			.replace(/\n/g, "\\n")
			.replace(/\t/g, "\\t") + '"';
	}

	function toJson(value) {
		var parts, key, i;
		if (value === null || value === undefined) {
			return "null";
		}
		if (typeof value === "boolean" || typeof value === "number") {
			return isFinite(value) || typeof value === "boolean" ? String(value) : "null";
		}
		if (typeof value === "string") {
			return quote(value);
		}
		if (value instanceof Array) {
			parts = [];
			for (i = 0; i < value.length; i++) {
				parts.push(toJson(value[i]));
			}
			return "[" + parts.join(",") + "]";
		}
		parts = [];
		for (key in value) {
			if (value.hasOwnProperty(key)) {
				parts.push(quote(key) + ":" + toJson(value[key]));
			}
		}
		return "{" + parts.join(",") + "}";
	}

	function describe(error) {
		var text = String(error);
		if (error && error.line) {
			text += " (trace.jsx line " + error.line + ")";
		}
		return text;
	}

	function writeResult() {
		var file = new File(cfg.result);
		file.encoding = "UTF-8";
		if (file.open("w")) {
			file.write(toJson(result));
			file.close();
		}
	}

	try {
		app.beginSuppressDialogs();

		var footage = app.project.importFile(new ImportOptions(new File(cfg.input)));
		folder = app.project.items.addFolder("aexlo probe run");
		footage.parentFolder = folder;

		var frameDuration = 1 / cfg.fps;
		var comp = app.project.items.addComp(
			"aexlo probe",
			footage.width,
			footage.height,
			1,
			frameDuration * cfg.times.length,
			cfg.fps
		);
		comp.parentFolder = folder;

		var layer = comp.layers.add(footage);
		var effect = layer.property("ADBE Effect Parade").addProperty(cfg.match_name);
		if (!effect) {
			throw new Error("effect " + cfg.match_name + " is not available; install the probe and restart After Effects");
		}

		for (var p = 0; p < cfg.params.length; p++) {
			var param = cfg.params[p];
			var property = effect.property(param.name);
			if (!property) {
				result.params.push({ name: param.name, ok: false, error: "no such parameter" });
				continue;
			}
			try {
				property.setValue(param.value);
				result.params.push({ name: param.name, ok: true, value: property.value });
			} catch (paramError) {
				result.params.push({ name: param.name, ok: false, error: describe(paramError) });
			}
		}

		originalDepth = app.project.bitsPerChannel;
		var depths = cfg.depths && cfg.depths.length ? cfg.depths : [null];
		for (var d = 0; d < depths.length; d++) {
			if (depths[d] !== null) {
				app.project.bitsPerChannel = depths[d];
			}
			for (var t = 0; t < cfg.times.length; t++) {
				var suffix = depths[d] === null ? String(t) : depths[d] + "_" + t;
				var out = new File(cfg.render_stem + "_" + suffix + ".png");
				comp.saveFrameToPng(cfg.times[t], out);
				result.renders.push(out.fsName);
			}
		}

		result.ok = true;
	} catch (error) {
		result.error = describe(error);
	}

	// Removing the folder removes the comp and the footage with it; the effect
	// instance goes away, so the probe sees SEQUENCE_SETDOWN.
	try {
		if (folder) {
			folder.remove();
		}
	} catch (cleanupError) {
		result.cleanup_error = describe(cleanupError);
	}

	try {
		if (originalDepth !== null) {
			app.project.bitsPerChannel = originalDepth;
		}
	} catch (depthError) {
		result.cleanup_error = describe(depthError);
	}

	try {
		app.endSuppressDialogs(false);
	} catch (suppressError) {
		result.cleanup_error = describe(suppressError);
	}

	writeResult();

	if (cfg.quit) {
		app.project.close(CloseOptions.DO_NOT_SAVE_CHANGES);
		app.quit();
	}
})();
