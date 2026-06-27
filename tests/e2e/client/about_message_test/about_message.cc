#include <cassert>
#include <iostream>

#include "AEConfig.h"
#include "AE_EffectCB.h"
#include "AE_Effect.h"

#ifdef _WIN
	#define DllExport __declspec( dllexport )
#else
	#define DllExport __attribute__ ((visibility ("default")))
#endif

PF_Err About(
	PF_InData *in_data,
	PF_OutData *out_data,
	PF_ParamDef *params[],
	PF_LayerDef *output
) {
	PF_Err error = PF_Err_NONE;

	in_data->utils->ansi.sprintf(
		out_data->return_msg,
		"%s",
		"Hello World!"
	);

	if (in_data->utils->ansi.strcpy == nullptr)
		return PF_Err_INTERNAL_STRUCT_DAMAGED;

	return error;
}

extern "C" {
	DllExport PF_Err EffectMain(
		PF_Cmd cmd,
		PF_InData *in_data,
		PF_OutData *out_data,
		PF_ParamDef *params[],
		PF_LayerDef *output,
		void *extra
	) {
		PF_Err error = PF_Err_NONE;

		try {
			switch (cmd) {
				case PF_Cmd_ABOUT:
					error = About(in_data, out_data, params, output);
					break;
			}
		} catch (PF_Err &thrown_error) {
			error = thrown_error;
		}

		return error;
	};
}
