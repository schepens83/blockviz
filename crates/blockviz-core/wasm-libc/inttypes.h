/* blockviz: what Prism uses of <inttypes.h>, for wasm32-unknown-unknown, which has
   no libc. Other targets get the system header. See src/ruby/libc.rs. */
#ifndef __wasm__
#include_next <inttypes.h>
#else
#pragma once
#include <stdint.h>
#define PRIu8 "u"
#define PRId8 "d"
#define PRIu16 "u"
#define PRId16 "d"
#define PRIi32 "i"
#define PRId32 "d"
#define PRIu32 "u"
#define PRIx32 "x"
#define PRIi64 "lli"
#define PRId64 "lld"
#define PRIu64 "llu"
#define PRIx64 "llx"
#endif
