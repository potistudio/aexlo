#include <cassert>
#include <iostream>

#include "AEConfig.h"
#include "AE_EffectCB.h"
#include "AE_Effect.h"

PF_Err About(
	PF_InData *in_data,
	PF_OutData *out_data,
	PF_ParamDef *params[],
	PF_LayerDef *output
) {
	PF_Err error = PF_Err_NONE;

	//== PF_InData::utils::ansi Functions ==//
	if (in_data->utils->ansi.atan == nullptr)
		return PF_Err_INTERNAL_STRUCT_DAMAGED;

	if (in_data->utils->ansi.atan2 == nullptr)
		return PF_Err_INTERNAL_STRUCT_DAMAGED;

	if (in_data->utils->ansi.ceil == nullptr)
		return PF_Err_INTERNAL_STRUCT_DAMAGED;

	if (in_data->utils->ansi.cos == nullptr)
		return PF_Err_INTERNAL_STRUCT_DAMAGED;

	if (in_data->utils->ansi.exp == nullptr)
		return PF_Err_INTERNAL_STRUCT_DAMAGED;

	if (in_data->utils->ansi.fabs == nullptr)
		return PF_Err_INTERNAL_STRUCT_DAMAGED;

	if (in_data->utils->ansi.floor == nullptr)
		return PF_Err_INTERNAL_STRUCT_DAMAGED;

	if (in_data->utils->ansi.fmod == nullptr)
		return PF_Err_INTERNAL_STRUCT_DAMAGED;

	if (in_data->utils->ansi.hypot == nullptr)
		return PF_Err_INTERNAL_STRUCT_DAMAGED;

	if (in_data->utils->ansi.log == nullptr)
		return PF_Err_INTERNAL_STRUCT_DAMAGED;

	if (in_data->utils->ansi.log10 == nullptr)
		return PF_Err_INTERNAL_STRUCT_DAMAGED;

	if (in_data->utils->ansi.pow == nullptr)
		return PF_Err_INTERNAL_STRUCT_DAMAGED;

	if (in_data->utils->ansi.sin == nullptr)
		return PF_Err_INTERNAL_STRUCT_DAMAGED;

	if (in_data->utils->ansi.sqrt == nullptr)
		return PF_Err_INTERNAL_STRUCT_DAMAGED;

	if (in_data->utils->ansi.sprintf == nullptr)
		return PF_Err_INTERNAL_STRUCT_DAMAGED;

	if (in_data->utils->ansi.strcpy == nullptr)
		return PF_Err_INTERNAL_STRUCT_DAMAGED;

	if (in_data->utils->ansi.asin == nullptr)
		return PF_Err_INTERNAL_STRUCT_DAMAGED;

	if (in_data->utils->ansi.acos == nullptr)
		return PF_Err_INTERNAL_STRUCT_DAMAGED;

	return error;
}

extern "C" {
	__declspec(dllexport) PF_Err EffectMain(
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
