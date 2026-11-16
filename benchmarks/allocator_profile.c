/* Native allocator diagnostics only: no HTTP or qualification claims. */
#include <vips/vips.h>
#include <mimalloc.h>
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#ifdef __linux__
#include <sys/resource.h>
#endif
#define SAMPLES 48
static double now(clockid_t clock) {
    struct timespec t;
    if (clock_gettime(clock, &t)) exit(2);
    return (double)t.tv_sec * 1000.0 + (double)t.tv_nsec / 1000000.0;
}
static int compare(const void *a, const void *b) {
    double x = *(const double *)a, y = *(const double *)b;
    return (x > y) - (x < y);
}
static int failure(int line) {
    fprintf(stderr, "Native allocator diagnostic failed at line %d: %s\n", line, vips_error_buffer());
    return 3;
}
static void release_bytes(gpointer bytes) { g_bytes_unref(bytes); }
static VipsImage *load(GBytes *bytes, int shrink, VipsAccess access) {
    VipsSource *source = vips_source_new_from_memory(g_bytes_get_data(bytes, NULL), g_bytes_get_size(bytes));
    if (!source) return NULL;
    g_object_set_data_full(G_OBJECT(source), "allocator-profile-bytes", g_bytes_ref(bytes), release_bytes);
    VipsImage *image = vips_image_new_from_source(source, "", "shrink", shrink,
        "access", access, "fail_on", VIPS_FAIL_ON_ERROR, NULL);
    g_object_unref(source);
    return image;
}
int main(int argc, char **argv) {
    if (argc != 3 || (strcmp(argv[2], "blur") && strcmp(argv[2], "sharpen"))) return 2;
#ifdef __linux__
    struct rlimit limit;
    if (getrlimit(RLIMIT_AS, &limit)) return 2;
    if (limit.rlim_cur > 1073741824) limit.rlim_cur = 1073741824;
    if (limit.rlim_max > 1073741824) limit.rlim_max = 1073741824;
    if (setrlimit(RLIMIT_AS, &limit)) return 2;
#endif
    if (VIPS_INIT("allocator-profile")) return 2;
    vips_cache_set_max(0); vips_cache_set_max_mem(0); vips_cache_set_max_files(0);
    vips_concurrency_set(1);
    gchar *source = NULL; gsize length = 0;
    if (!g_file_get_contents(argv[1], &source, &length, NULL) || length > 64 * 1024 * 1024) return 2;
    double wall[SAMPLES], cpu[SAMPLES], sum = 0;
    gchar *digest = NULL;
    for (int sample = -6; sample < SAMPLES; sample++) {
        double begin = now(CLOCK_MONOTONIC), cpu_begin = now(CLOCK_PROCESS_CPUTIME_ID);
        GBytes *bytes = g_bytes_new_take(g_memdup2(source, length), length);
        VipsImage *head = load(bytes, 1, VIPS_ACCESS_RANDOM);
        if (!head) return failure(__LINE__);
        int w = head->Xsize, h = head->Ysize;
        if (w <= 0 || h <= 0 || w > 100000 || h > 100000 || (int64_t)w * h > 50000000 ||
            head->BandFmt != VIPS_FORMAT_UCHAR || head->Bands != 3 ||
            head->Type != VIPS_INTERPRETATION_sRGB || vips_image_get_typeof(head, "icc-profile-data")) return failure(__LINE__);
        double factor = fmin(1.0, fmin(640.0 / w, 360.0 / h));
        int shrink = factor <= .125 ? 8 : factor <= .25 ? 4 : factor <= .5 ? 2 : 1;
        VipsImage *input = load(bytes, shrink, VIPS_ACCESS_SEQUENTIAL);
        g_bytes_unref(bytes);
        g_object_unref(head);
        if (!input) return failure(__LINE__);
        VipsImage *resized = NULL, *filtered = NULL, *stream = NULL;
        if (vips_resize(input, &resized, factor * w / input->Xsize, "vscale", factor * h / input->Ysize,
            "kernel", VIPS_KERNEL_LANCZOS3, NULL)) return failure(__LINE__);
        g_object_unref(input);
        int error = !strcmp(argv[2], "sharpen")
            ? vips_sharpen(resized, &filtered, "sigma", 1.0, NULL)
            : vips_gaussblur(resized, &filtered, 2.0, NULL);
        g_object_unref(resized);
        if (error || vips_sequential(filtered, &stream, "tile_height", 16, NULL)) return failure(__LINE__);
        g_object_unref(filtered);
        void *output = NULL; size_t size = 0;
        if (vips_jpegsave_buffer(stream, &output, &size, "Q", 85, "strip", TRUE, NULL)) return failure(__LINE__);
        g_object_unref(stream);
        vips_error_clear(); vips_thread_shutdown();
        double elapsed = now(CLOCK_MONOTONIC) - begin, used = now(CLOCK_PROCESS_CPUTIME_ID) - cpu_begin;
        /* Verify every body outside timing, then include response deallocation. */
        gchar *hash = g_compute_checksum_for_data(G_CHECKSUM_SHA256, output, size);
        if (!digest) digest = g_strdup(hash);
        if (strcmp(digest, hash)) return 4;
        g_free(hash);
        begin = now(CLOCK_MONOTONIC); cpu_begin = now(CLOCK_PROCESS_CPUTIME_ID);
        g_free(output);
        elapsed += now(CLOCK_MONOTONIC) - begin; used += now(CLOCK_PROCESS_CPUTIME_ID) - cpu_begin;
        if (sample >= 0) { wall[sample] = elapsed; cpu[sample] = used; sum += used; }
    }
    qsort(wall, SAMPLES, sizeof(double), compare);
    qsort(cpu, SAMPLES, sizeof(double), compare);
    unsigned long long peak = 0;
#ifdef __linux__
    FILE *counter = fopen("/sys/fs/cgroup/memory.peak", "r");
    if (!counter || fscanf(counter, "%llu", &peak) != 1) return 5;
    fclose(counter);
#endif
    printf("{\"operation\":\"%s\",\"samples\":%d,\"allocator_version\":%d,"
        "\"processing_p50_ms\":%.6f,\"processing_p95_ms\":%.6f,\"cpu_mean_ms\":%.6f,"
        "\"cgroup_peak_memory_bytes\":%llu,\"output_sha256\":\"%s\"}\n",
        argv[2], SAMPLES, mi_version(), wall[(SAMPLES - 1) / 2], wall[(95 * SAMPLES + 99) / 100 - 1],
        sum / SAMPLES, peak, digest);
    g_free(source); g_free(digest); vips_shutdown(); return 0;
}
