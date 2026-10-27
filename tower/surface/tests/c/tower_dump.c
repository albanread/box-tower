/* tower_dump.c -- read a live block, an art block and a surface record
 * written by tower-surface (tests/layout.rs) through rosgd/tower.h, and
 * print every field, so that the test can compare them with what Rust
 * wrote.  Also prints the sizes and offsets the C compiler gives.
 *
 *   tower_dump layout
 *   tower_dump live <file> | art <file> | surf <file>                       */
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include <rosgd/tower.h>

static void *slurp(const char *path, size_t want)
{
    FILE *f = fopen(path, "rb");
    if (!f) { perror(path); exit(2); }
    void *p = calloc(1, want);
    if (!p || fread(p, 1, want, f) != want) { fprintf(stderr, "%s: short\n", path); exit(2); }
    fclose(f);
    return p;
}

#define OFF(T, f) printf("%s.%s %zu\n", #T, #f, offsetof(struct T, f))

static void layout(void)
{
    printf("sizeof ros_tower %zu\n", sizeof(struct ros_tower));
    printf("sizeof ros_tower_art %zu\n", sizeof(struct ros_tower_art));
    printf("sizeof ros_surf_tower %zu\n", sizeof(struct ros_surf_tower));
    printf("sizeof ros_tower_cell %zu\n", sizeof(struct ros_tower_cell));
    printf("sizeof ros_tower_room %zu\n", sizeof(struct ros_tower_room));
    printf("sizeof ros_tower_fixture %zu\n", sizeof(struct ros_tower_fixture));
    printf("sizeof ros_tower_mover %zu\n", sizeof(struct ros_tower_mover));
    OFF(ros_tower, magic); OFF(ros_tower, version); OFF(ros_tower, frame); OFF(ros_tower, art);
    OFF(ros_tower, grid_w); OFF(ros_tower, grid_h); OFF(ros_tower, ground_row); OFF(ros_tower, scale);
    OFF(ros_tower, nrooms); OFF(ros_tower, nfixtures); OFF(ros_tower, nmovers);
    OFF(ros_tower, quality); OFF(ros_tower, rate); OFF(ros_tower, view); OFF(ros_tower, flags);
    OFF(ros_tower, hour); OFF(ros_tower, day); OFF(ros_tower, outline); OFF(ros_tower, reserved0);
    OFF(ros_tower, outline_rgba); OFF(ros_tower, back_rgba); OFF(ros_tower, gen);
    OFF(ros_tower, room_gen); OFF(ros_tower, reserved); OFF(ros_tower, cell); OFF(ros_tower, room);
    OFF(ros_tower, fix); OFF(ros_tower, stops); OFF(ros_tower, pal); OFF(ros_tower, mover);
    OFF(ros_tower_cell, room); OFF(ros_tower_cell, back); OFF(ros_tower_cell, front); OFF(ros_tower_cell, flags);
    OFF(ros_tower_mover, x); OFF(ros_tower_mover, y); OFF(ros_tower_mover, spr); OFF(ros_tower_mover, param);
    OFF(ros_tower_art, magic); OFF(ros_tower_art, version); OFF(ros_tower_art, gen); OFF(ros_tower_art, size);
    OFF(ros_tower_art, scale); OFF(ros_tower_art, atlas_w); OFF(ros_tower_art, atlas_h);
    OFF(ros_tower_art, spr_w); OFF(ros_tower_art, spr_h); OFF(ros_tower_art, nart);
    OFF(ros_tower_art, nsprites); OFF(ros_tower_art, emit_shift); OFF(ros_tower_art, flags);
    OFF(ros_tower_art, lang); OFF(ros_tower_art, reserved); OFF(ros_tower_art, art);
    OFF(ros_tower_art, spr); OFF(ros_tower_art, ramp); OFF(ros_tower_art, prof);
    OFF(ros_surf_tower, backing); OFF(ros_surf_tower, offset); OFF(ros_surf_tower, size);
    OFF(ros_surf_tower, frame); OFF(ros_surf_tower, art_backing); OFF(ros_surf_tower, art_offset);
    OFF(ros_surf_tower, art_size); OFF(ros_surf_tower, art_gen);
}

static void live(const struct ros_tower *t)
{
    printf("magic %u\nversion %u\nframe %u\nart %u\n", t->magic, t->version, t->frame, t->art);
    printf("grid %u %u\nground_row %u\nscale %u\n", t->grid_w, t->grid_h, t->ground_row, t->scale);
    printf("counts %u %u %u\n", t->nrooms, t->nfixtures, t->nmovers);
    printf("bytes %u %u %u %u\n", t->quality, t->rate, t->view, t->flags);
    printf("hour %u\nday %u\n", t->hour, t->day);
    printf("outline %d %d %d %d\n", t->outline[0], t->outline[1], t->outline[2], t->outline[3]);
    printf("reserved0 %u\noutline_rgba %u\nback_rgba %u\n", t->reserved0, t->outline_rgba, t->back_rgba);
    for (int i = 0; i < 8; i++) printf("gen[%d] %u\n", i, t->gen[i]);
    for (int i = 0; i < 16; i++) printf("room_gen[%d] %u\n", i, t->room_gen[i]);
    for (int i = 0; i < 10; i++) printf("reserved[%d] %u\n", i, t->reserved[i]);
    for (int y = 0; y < ROS_TW_GRID_H; y++)
        for (int x = 0; x < ROS_TW_STRIDE; x++) {
            const struct ros_tower_cell *c = &t->cell[y][x];
            printf("cell[%d][%d] %u %u %u %u\n", y, x, c->room, c->back, c->front, c->flags);
        }
    for (int i = 0; i < ROS_TW_ROOMS; i++)
        printf("room[%d] %u %u %u %u\n", i, t->room[i].w[0], t->room[i].w[1], t->room[i].w[2], t->room[i].w[3]);
    for (int i = 0; i < ROS_TW_FIXTURES; i++)
        printf("fix[%d] %u %u %u %u\n", i, t->fix[i].w[0], t->fix[i].w[1], t->fix[i].w[2], t->fix[i].w[3]);
    for (int s = 0; s < ROS_TW_SHAFTS; s++)
        for (int r = 0; r < ROS_TW_ROWS; r++) printf("stops[%d][%d] %u\n", s, r, t->stops[s][r]);
    for (int i = 0; i < 16; i++) printf("pal[%d] %u\n", i, t->pal[i]);
    for (int i = 0; i < ROS_TW_MOVERS; i++) {
        const struct ros_tower_mover *m = &t->mover[i];
        printf("mover[%d] %u %u %u %u\n", i, m->x, m->y, m->spr, m->param);
    }
}

static void art(const struct ros_tower_art *a)
{
    printf("magic %u\nversion %u\ngen %u\nsize %u\n", a->magic, a->version, a->gen, a->size);
    printf("scale %u\natlas %u %u\nspr %u %u\n", a->scale, a->atlas_w, a->atlas_h, a->spr_w, a->spr_h);
    printf("counts %u %u\nemit_shift %u\nflags %u\nlang %u\n", a->nart, a->nsprites, a->emit_shift,
           a->flags, a->lang);
    for (int i = 0; i < 7; i++) printf("reserved[%d] %u\n", i, a->reserved[i]);
    for (int i = 0; i < ROS_TW_ART; i++)
        printf("art[%d] %u %u %u %u\n", i, a->art[i][0], a->art[i][1], a->art[i][2], a->art[i][3]);
    for (int i = 0; i < ROS_TW_SPRITES; i++)
        printf("spr[%d] %u %u %u %u\n", i, a->spr[i][0], a->spr[i][1], a->spr[i][2], a->spr[i][3]);
    for (int r = 0; r < 4; r++)
        for (int c = 0; c < ROS_TW_RAMP; c++)
            printf("ramp[%d][%d] %u %u %u %u\n", r, c, a->ramp[r][c][0], a->ramp[r][c][1],
                   a->ramp[r][c][2], a->ramp[r][c][3]);
    for (int d = 0; d < 2; d++)
        for (int p = 0; p < ROS_TW_PROFILES; p++)
            for (int h = 0; h < 24; h++) printf("prof[%d][%d][%d] %u\n", d, p, h, a->prof[d][p][h]);
    printf("at %llu %llu %llu %llu %llu %llu\n",
           (unsigned long long)ros_tower_art_albedo(a), (unsigned long long)ros_tower_art_emis(a),
           (unsigned long long)ros_tower_art_aux(a), (unsigned long long)ros_tower_art_sprites(a),
           (unsigned long long)ros_tower_art_spremis(a), (unsigned long long)ros_tower_art_size(a));
}

static void surf(const struct ros_surf_tower *s)
{
    printf("live %u %u %u %u\n", s->backing, s->offset, s->size, s->frame);
    printf("art %u %u %u %u\n", s->art_backing, s->art_offset, s->art_size, s->art_gen);
}

int main(int argc, char **argv)
{
    if (argc == 2 && !strcmp(argv[1], "layout")) { layout(); return 0; }
    if (argc != 3) { fprintf(stderr, "usage: tower_dump layout | live|art|surf <file>\n"); return 2; }
    if (!strcmp(argv[1], "live")) live(slurp(argv[2], sizeof(struct ros_tower)));
    else if (!strcmp(argv[1], "art")) art(slurp(argv[2], sizeof(struct ros_tower_art)));
    else if (!strcmp(argv[1], "surf")) surf(slurp(argv[2], sizeof(struct ros_surf_tower)));
    else return 2;
    return 0;
}
