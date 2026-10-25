/* blockviz: what Prism uses of <unistd.h>, for wasm32-unknown-unknown, which has
   no libc. Other targets get the system header. See src/ruby/libc.rs. */
#ifndef __wasm__
#include_next <unistd.h>
#else
#pragma once
/* Prism includes this first to ask about files; there are none to map or
   read here, it is handed sources in memory. */
#define PRISM_HAS_NO_FILESYSTEM
#endif
