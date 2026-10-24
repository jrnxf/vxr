// Prints an ogt_vox reading of a .vox file in the same format as `voxtool dump`,
// so the two readers can be diffed. Build: see harness/build-ogt.
#define OGT_VOX_IMPLEMENTATION
#include "ogt_vox.h"
#include <cstdio>
#include <cstdint>
#include <vector>

static void fnv(uint64_t& h, uint8_t b) { h ^= b; h *= 0x100000001b3ull; }

int main(int argc, char** argv) {
    if (argc < 2) { fprintf(stderr, "usage: ogt_dump file.vox\n"); return 2; }
    FILE* f = fopen(argv[1], "rb");
    if (!f) { perror("open"); return 2; }
    std::vector<uint8_t> buf;
    uint8_t tmp[65536];
    size_t n;
    while ((n = fread(tmp, 1, sizeof tmp, f)) > 0) buf.insert(buf.end(), tmp, tmp + n);
    fclose(f);
    uint32_t flags = k_read_scene_flags_keep_empty_models_instances | k_read_scene_flags_keep_duplicate_models;
    const ogt_vox_scene* s = ogt_vox_read_scene_with_flags(buf.data(), (uint32_t)buf.size(), flags);
    if (!s) { printf("rejected\n"); return 1; }
    printf("version %u\n", s->file_version);
    printf("models %u\n", s->num_models);
    for (uint32_t i = 0; i < s->num_models; i++) {
        const ogt_vox_model* m = s->models[i];
        if (!m) { printf("model %u null\n", i); continue; }
        uint64_t h = 0xcbf29ce484222325ull;
        uint32_t count = 0;
        for (uint32_t x = 0; x < m->size_x; x++)
            for (uint32_t y = 0; y < m->size_y; y++)
                for (uint32_t z = 0; z < m->size_z; z++) {
                    uint8_t c = m->voxel_data[x + y * m->size_x + z * m->size_x * m->size_y];
                    if (!c) continue;
                    count++;
                    ogt_vox_rgba col = s->palette.color[c];
                    uint8_t b[7] = {(uint8_t)x, (uint8_t)y, (uint8_t)z, col.r, col.g, col.b, col.a};
                    for (uint8_t v : b) fnv(h, v);
                }
        printf("model %u size %u %u %u voxels %u hash %016llx\n", i, m->size_x, m->size_y, m->size_z, count, (unsigned long long)h);
    }
    printf("instances %u\n", s->num_instances);
    for (uint32_t i = 0; i < s->num_instances; i++) {
        const ogt_vox_instance& in = s->instances[i];
        const ogt_vox_transform& t = in.transform;
        // ogt stores MagicaVoxel's rows as columns; print rows to match voxtool.
        printf("instance model %u r %d %d %d %d %d %d %d %d %d t %d %d %d layer %d hidden %d\n",
               in.model_index,
               (int)t.m00, (int)t.m10, (int)t.m20, (int)t.m01, (int)t.m11, (int)t.m21, (int)t.m02, (int)t.m12, (int)t.m22,
               (int)t.m30, (int)t.m31, (int)t.m32, (int32_t)in.layer_index, in.hidden ? 1 : 0);
    }
    ogt_vox_destroy_scene(s);
    return 0;
}
