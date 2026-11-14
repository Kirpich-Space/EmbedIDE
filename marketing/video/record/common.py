"""Shared helpers for the recording scripts: a throw-away demo project and a clean environment."""
import os, shutil, sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))
from lib import term as T  # noqa: E402

REPO = HERE.parents[2]
BINARY = Path(os.environ.get("EMBED_BIN", REPO / "target" / "release" / "embed"))
OUT = HERE.parent / "out"
DEMO = Path("/tmp/demo")


def env(cfg, extra=None):
    e = {k: v for k, v in os.environ.items()
         if not k.startswith(("CLAUDE", "ANTHROPIC", "OPENAI", "CODEX_API", "DISPLAY", "WAYLAND"))}
    e.update(TERM="xterm-256color", COLORTERM="truecolor", SHELL="/bin/sh", EMBEDIDE_CONFIG_DIR=str(cfg),
             LANG="C.UTF-8", PS1="$ ")
    e.update(extra or {})
    return e


RING_H = """#ifndef RING_H
#define RING_H
#include <stdbool.h>
#include <stddef.h>

#define RING_CAPACITY 8

typedef struct {
    int items[RING_CAPACITY];
    size_t head;
    size_t count;
} ring_t;

void ring_init(ring_t *r);
bool ring_push(ring_t *r, int value);
bool ring_pop(ring_t *r, int *out);

#endif
"""

# The deliberate mistake for the demo: a missing semicolon after `r->count--`.
RING_C_BROKEN = """#include "ring.h"

void ring_init(ring_t *r)
{
    r->head = 0;
    r->count = 0;
}

bool ring_push(ring_t *r, int value)
{
    if (r->count == RING_CAPACITY)
        return false;
    r->items[(r->head + r->count) % RING_CAPACITY] = value;
    r->count++;
    return true;
}

bool ring_pop(ring_t *r, int *out)
{
    if (r->count == 0)
        return false;
    *out = r->items[r->head];
    r->head = (r->head + 1) % RING_CAPACITY;
    r->count--
    return true;
}
"""

MAIN_C = """#include <stdio.h>
#include "ring.h"

int main(void)
{
    ring_t ring;
    ring_init(&ring);

    for (int i = 1; i <= 5; i++)
        ring_push(&ring, i * 10);

    int value;
    while (ring_pop(&ring, &value))
        printf("popped %d\\n", value);

    return 0;
}
"""

TOML = """schema = 1

[project]
name = "ringbuf"
language = "c"

[build]
system = "native"
sources = ["src"]
flags = ["-Wall", "-Wextra"]

[toolchain]
c = "gcc"

[profiles.debug]
flags = ["-O0", "-g"]
"""


def make_project(name="ringbuf", broken=True):
    root = DEMO / name
    shutil.rmtree(root, ignore_errors=True)
    (root / "src").mkdir(parents=True)
    (root / "src" / "ring.h").write_text(RING_H)
    (root / "src" / "ring.c").write_text(RING_C_BROKEN if broken else RING_C_BROKEN.replace("r->count--\n", "r->count--;\n"))
    (root / "src" / "main.c").write_text(MAIN_C)
    (root / "embedide.toml").write_text(TOML)
    return root


def fresh_cfg(name):
    cfg = DEMO / f"cfg-{name}"
    shutil.rmtree(cfg, ignore_errors=True)
    cfg.mkdir(parents=True)
    return cfg
