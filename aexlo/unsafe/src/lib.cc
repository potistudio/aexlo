#include <iostream>

extern "C" {
	int Iterate8 (int a, int b) {
		std::cout << "Iterate8 called with " << a << " and " << b << std::endl;
		return a + b;
	}
}
