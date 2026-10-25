/* blockviz: what Prism uses of <math.h>, for wasm32-unknown-unknown, which has
   no libc. Other targets get the system header. See src/ruby/libc.rs. */
#ifndef __wasm__
#include_next <math.h>
#else
#pragma once
#define HUGE_VAL __builtin_huge_val()
#define isinf(x) __builtin_isinf(x)
#define isnan(x) __builtin_isnan(x)
#endif
