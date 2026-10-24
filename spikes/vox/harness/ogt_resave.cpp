// Re-saves a .vox file through ogt_vox (read with groups and keyframes, write back).
// Stands in for "a different writer re-saved our file" when testing `voxtool compare`.
#define OGT_VOX_IMPLEMENTATION
#include "ogt_vox.h"
#include <cstdio>
#include <cstdint>
#include <vector>

int main(int argc, char** argv) {
    if (argc < 3) { fprintf(stderr, "usage: ogt_resave in.vox out.vox\n"); return 2; }
    FILE* f = fopen(argv[1], "rb");
    if (!f) { perror("open"); return 2; }
    std::vector<uint8_t> buf;
    uint8_t tmp[65536];
    size_t n;
    while ((n = fread(tmp, 1, sizeof tmp, f)) > 0) buf.insert(buf.end(), tmp, tmp + n);
    fclose(f);
    uint32_t flags = k_read_scene_flags_groups | k_read_scene_flags_keyframes |
                     k_read_scene_flags_keep_empty_models_instances | k_read_scene_flags_keep_duplicate_models;
    const ogt_vox_scene* s = ogt_vox_read_scene_with_flags(buf.data(), (uint32_t)buf.size(), flags);
    if (!s) { fprintf(stderr, "rejected\n"); return 1; }
    uint32_t size = 0;
    uint8_t* out = ogt_vox_write_scene(s, &size);
    FILE* o = fopen(argv[2], "wb");
    fwrite(out, 1, size, o);
    fclose(o);
    ogt_vox_free(out);
    ogt_vox_destroy_scene(s);
    return 0;
}
