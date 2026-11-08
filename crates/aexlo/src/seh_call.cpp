/* Catches a structured exception that escapes a plugin entry point.
 * A C++ exception crossing a Rust frame aborts the process, so the call
 * itself lives here, where it becomes an error code instead. */

#define WIN32_LEAN_AND_MEAN
#include <windows.h>

#include <stdint.h>

typedef int32_t(__cdecl *AexloPluginEntry)(int32_t, void *, void *, void *, void *, void *);

static int seh_filter(unsigned int code, uint32_t *raised) {
	if (raised != 0) {
		*raised = code;
	}
	return EXCEPTION_EXECUTE_HANDLER;
}

extern "C" int32_t aexlo_call_entry(void *entry, int32_t cmd, void *in_data, void *out_data, void *params,
									void *output, void *extra, uint32_t *raised) {
	if (raised != 0) {
		*raised = 0;
	}
	if (entry == 0) {
		return 0;
	}
	__try {
		return ((AexloPluginEntry)entry)(cmd, in_data, out_data, params, output, extra);
	} __except (seh_filter(GetExceptionCode(), raised)) {
		return 0;
	}
}
