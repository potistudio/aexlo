#include <iostream>

// Represent a 8-bit pixel that is used in AE
struct Pixel {
	uint8_t a, r, g, b;
};

extern "C" {
	void Iterate8(
		int    pixel_count,
		Pixel* in_layer,
		Pixel* out_layer,
		void*  controller,
		int    (*pix_fn)(void* controller, int x, int y, Pixel* in, Pixel* out)
	) {
		#pragma omp parallel for
		for (int i = 0; i < pixel_count; ++i) {
			pix_fn(controller, i % 1920, i / 1920, &in_layer[i], &out_layer[i]);
		}
	}
}
