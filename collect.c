// Collect all stdin into memory, then 
#define _GNU_SOURCE

#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <stdbool.h>

#include <unistd.h>
#include <sys/stat.h>
#include <fcntl.h>
#include <sys/mman.h>

#include <errno.h>

#define LIKELY(expr) __builtin_expect(!!(expr), true)
#define UNLIKELY(expr) __builtin_expect(!!(expr), false)

#define _$_if__L true
#define _$_if__U false
#define $if(l, expr) if(__builtin_expect(!!(expr), _$_if__ ## l))

#define F_STDIN 0
#define F_STDOUT 1
#define F_STDERR 2

typedef union arguments {
	struct {
		off_t pages_per_buffer;
	} sized;
	struct {
		size_t buffsz;
	} unsized;
} option_t;

#define DEFAULT_OPTION ((option_t){ .sized = { .pages_per_buffer = 8 } })

static bool has_size(int fd, off_t* restrict size)
{
	struct stat st;
	if( fstat(fd, &st) < 0 ) { perror("failed to stat stdin"); return false; }
	else if (st.st_size > 0) {
		// Non-zero size
		*size = st.st_size;
		return true;
	}
	fprintf(stderr, "returned sz (fd %d): %ld\n", fd, st.st_size);
	return false;
}

int collect_sized(off_t sz, const option_t* opt);
int collect_unsized(const option_t* opt);

int main(void)
{
	off_t sz;
	option_t args = DEFAULT_OPTION;
	if(has_size(F_STDIN, &sz)) {
		return collect_sized((size_t)sz, &args);
	} else {
		return collect_unsized(&args);
	}
}


inline static
const void* map_input_buffer(int fd, size_t sz)
{
	void* map = mmap(NULL, sz, PROT_READ, MAP_PRIVATE, fd, 0);

	if(UNLIKELY(map == MAP_FAILED)) {
		perror("input mmap()");
		return NULL;
	}

	return map;	
}

inline static
bool unmap_mem(void* mem, size_t len)
{
	if(UNLIKELY( munmap(mem, len) != 0 )) {
		perror("munmap()");
		return false;
	}
	return true;
}

static int page_size()
{
	static int _page_size=0;
	if(UNLIKELY(!_page_size)) return _page_size = getpagesize();
	return _page_size;
}

inline static
bool alloc_pages(off_t pages, int *restrict _fd, size_t* restrict _size)
{
	int fd = memfd_create("collect-sized-buffer", O_RDWR);
	$if(U, fd < 0) goto _e_memfd;
	$if(U, fallocate(fd, 0, 0, __builtin_constant_p(_size) && !_size 
					? pages * page_size() 
					: _size ? (off_t)( *_size = pages * page_size() )
					: pages * page_size()) != 0) goto _e_fallocate;
	$if(L, _fd) *_fd = fd;
	else close(fd);

	return true;
	// +Unwind+ //
_e_fallocate:
	perror("fallocate()");
	close(fd);
	if(0)
_e_memfd:
	perror("memfd_create()");
	// -Unwind- //	
	return false;
}

struct map_fd {
	void* map;
	size_t len;
	int fd;
};

static
bool map_pages(off_t pages, struct map_fd* restrict out)
{
	$if(U, !out) return alloc_pages(pages, NULL, NULL);

	$if(U, !alloc_pages(pages, &out->fd, &out->len)) goto _e_ap;
	$if(U, (out->map = mmap(NULL, out->len, PROT_READ|PROT_WRITE, MAP_PRIVATE, out->fd, 0)) == MAP_FAILED) goto _e_map;
	$if(U, madvise(out->map, out->len, MADV_MERGEABLE | MADV_WILLNEED)) goto _e_madv;

	return true;

	// +Unwind+ //
_e_madv:
	perror("madv()");
	munmap(out->map, out->len);
	if(0)
_e_map:
	perror("mmap()");
	close(out->fd);
	if(0)
_e_ap:
	(void)0; // no perror() needed
	// -Unwind- //
	return false;
}

inline static
void unmap_pages(struct map_fd in, int *restrict keep_fd)
{
	$if(U, munmap(in.map, in.len)) perror("munmap()");
	if(__builtin_constant_p(keep_fd) && keep_fd) *keep_fd = in.fd;
	else {
		if(!keep_fd) {
			$if(U, close(in.fd)) perror("close()");
		} else *keep_fd = in.fd;
	}
}

int collect_sized(off_t isz, const option_t* gopt)
{
	register int rc=0;
	__auto_type opt = gopt->sized;
	const off_t real_max_size = page_size() * opt.pages_per_buffer;
//	const off_t pages_per_isz = isz % page_size();
//	const off_t page_leftover_isz = isz / page_size();

	struct map_fd buffer;
	if(!map_pages(opt.pages_per_buffer, &buffer)) return 1;

	if(isz > real_max_size) {
		// Multiple buffers needed
	} else $if(U, isz == real_max_size) {
		// Exactly one buffer (unlikely, but possible)
		ssize_t r = splice(F_STDIN, NULL,
				buffer.fd, NULL,
				(size_t)isz,
				SPLICE_F_MOVE);
		switch(r) {
			case -1: goto _e_splice;
			case 0: /* TODO: splice reported end-of-input, should we ignore this? */
				rc = 10;
				goto _cleanup_splice;
			default: {
				fprintf(stderr, "splice()'d %lu bytes into buffer (%ld size @ %d)\n", r, buffer.len, buffer.fd);
			}
			break;
		}
		//TODO: splice() all bytes from that buffer into STDOUT
		rc = 0;
	} else {
		// Less than one buffer
		ssize_t r = splice(F_STDIN, NULL, // TODO: XXX: WHY does splice() **ALWAYS** fail??? it literally never works???
				buffer.fd, NULL,
				(size_t)isz,
				SPLICE_F_MOVE);
		switch(r) {
			case -1: goto _e_splice;
			case 0: /* TODO: splice reported end-of-input, should we ignore this? */
				rc = 10;
				goto _cleanup_splice;
			default: {
				fprintf(stderr, "splice()'d %lu bytes into buffer (%ld size @ %d)\n", r, buffer.len, buffer.fd);
			}
			break;
		}
		// TODO: splice() isz bytes from buffer into stdout
		rc = 0;
	}

	// +Cleanup+ //
_cleanup_splice: if(0)
_e_splice: rc = (perror("splice()"), -1);
	unmap_pages(buffer, NULL);
	// -Cleanup- //
	return rc;
}

int collect_unsized(const option_t* opt)
{
	return 0;
}

#if 0
int collect_sized(off_t isz, const option_t* opt)
{
	const size_t sz = (size_t)isz;
	fprintf(stderr, "size of input: %lu, max size of mapping: %lu (buffers %lu / lo %lu)\n", sz, opt->sized.maxsz, 
			sz % opt->sized.maxsz,
			sz / opt->sized.maxsz);

	//fcntl(F_STDOUT, ... SOMETHING to make splice() work here...
	//TODO :: XXX: : WHY can't we splice() here???? w/e..
#if 1
	if( fallocate(F_STDOUT, 0 /* | FALLOC_FL_KEEP_SIZE*/, 0, isz) != 0) {
		perror("fallocate(STDOUT)");
//		return 1;
	}
#endif

	if( fcntl(F_STDOUT, F_SETFL, fcntl(F_STDOUT, F_GETFL) & ~O_APPEND) < 0 )
	{
		perror("fcntl(stdout) + O_APPEND");
		return -O_APPEND;
	}
	ssize_t sprc = splice(F_STDIN, NULL, 
		F_STDOUT, NULL, //TODO: XXX: Why does this always fail? I've seen splice(1, 2) work before...
		sz, 
		SPLICE_F_MOVE);
	switch(sprc) {
		case -1: perror("splice() whole buffer failed");
			return 1;
		case 0:
			fprintf(stderr, "splice() reported end-of-input. TODO: continue splicing, or ignore?\n");
			return 2;
		default:
			if((size_t)sprc == sz) return 0;
			else if (sprc < sz) {
				fprintf(stderr, "splice() moved only %ld / %lu bytes. TODO: move the other %lu bytes\n",
					sprc, sz,
					sz - (size_t)sprc);
				return 3;
			} else if(sprc > sz) fprintf(stderr, "splice() somehow moved %ld / %lu (+ %ld bytes more)\n",
					sprc, sz,
					(size_t)sprc - sz);
			return -1;
	}
#if 0
	// Map stdin
	const void* stdin_map = map_input_buffer(F_STDIN, sz);
	if(!stdin_map) goto e_map_input;



cleanup:

	unmap_mem((void*)stdin_map, sz);
	if(0)
e_map_input:
	{ fprintf(stderr, "failed to map stdin (%lu)\n", sz); rc = 1; }
	return rc;
#endif
}

#endif
