/* blockviz: what Prism uses of <errno.h>, for wasm32-unknown-unknown, which has
   no libc. Other targets get the system header. See src/ruby/libc.rs. */
#ifndef __wasm__
#include_next <errno.h>
#else
#pragma once
/* Only Prism's number parsing reads it, in the file that sets it. */
static int pm_wasm_errno __attribute__((unused));
#define errno pm_wasm_errno
#define ERANGE 34
#endif
