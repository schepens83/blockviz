/* blockviz: what Prism uses of <assert.h>, for wasm32-unknown-unknown, which has
   no libc. Other targets get the system header. See src/ruby/libc.rs. */
#ifndef __wasm__
#include_next <assert.h>
#else
#pragma once
#define assert(condition) ((void) 0)
#endif
