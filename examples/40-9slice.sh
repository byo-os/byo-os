#!/bin/bash
# 9-Slice Background Images — Stretch vs Tile, 1x vs HiDPI
#
# Demonstrates:
#   1. background-image-slice with stretch mode (default)
#   2. background-image-slice with tile mode
#   3. Top row: 1x only images (no multi-density)
#   4. Bottom row: 1x + 2x images (HiDPI)
#
# Run from the project root:  ./examples/40-9slice.sh

# ─── Kitty Graphics upload helpers ────────────────────────────────────────────

send_chunked() {
    local first="y" id="$1"
    while IFS= read -r chunk || [ -n "$chunk" ]; do
        if [ "$first" = "y" ]; then
            printf '\e_Ga=t,f=100,q=1,i=%d,m=1;%s\e\\' "$id" "$chunk"
            first="n"
        else
            printf '\e_Gq=1,m=1;%s\e\\' "$chunk"
        fi
    done
    [ "$first" = "n" ] && printf '\e_Gq=2,m=0;\e\\'
}

transmit_png() {
    local id="$1" file="$2"
    { base64 -w 4096 "$file" 2>/dev/null | send_chunked "$id"; } ||
    { base64 -b 4096 "$file" 2>/dev/null | send_chunked "$id"; } ||
    { openssl base64 -e -A -in "$file" | fold -b -w 4096 | send_chunked "$id"; }
}

# ─── Phase 1: Upload images ──────────────────────────────────────────────────
# Image 1: 1x 9-slice (256x256)
# Image 2: 2x 9-slice (512x512)

transmit_png 1 assets/9-slice.png
transmit_png 2 assets/9-slice@2x.png

sleep 0.1

# ─── Phase 2: Build the UI ───────────────────────────────────────────────────
# Layout:
#   ┌─────────────────────────────────────────┐
#   │  1x Only                                │
#   │  ┌─── Stretch ───┐  ┌──── Tile ────┐    │
#   │  │               │  │              │    │
#   │  └───────────────┘  └──────────────┘    │
#   │  1x + 2x (HiDPI)                        │
#   │  ┌─── Stretch ───┐  ┌──── Tile ────┐    │
#   │  │               │  │              │    │
#   │  └───────────────┘  └──────────────┘    │
#   └─────────────────────────────────────────┘

SLICE=85

printf '\e_B
  +view root class="flex flex-col w-full h-full bg-zinc-900 p-8 gap-6" {

    +text title content="9-Slice Background Images" class="text-xl text-white"

    +text label-1x content="1x Only (single image)" class="text-sm text-zinc-400"

    +view row1 class="flex gap-6" {

      +view col1 class="flex flex-col gap-2" {
        +text l1 content="Stretch" class="text-xs text-zinc-500"
        +view stretch-1x class="rounded-xl overflow-clip"
            width=400 height=200
            background-image=$img(1)
            background-image-slice=%d
      }

      +view col2 class="flex flex-col gap-2" {
        +text l2 content="Tile" class="text-xs text-zinc-500"
        +view tile-1x class="rounded-xl overflow-clip"
            width=400 height=200
            background-image=$img(1)
            background-image-slice=%d
            background-image-repeat=tile
      }
    }

    +text label-2x content="1x + 2x (HiDPI — should look identical at 2x)" class="text-sm text-zinc-400"

    +view row2 class="flex gap-6" {

      +view col3 class="flex flex-col gap-2" {
        +text l3 content="Stretch" class="text-xs text-zinc-500"
        +view stretch-2x class="rounded-xl overflow-clip"
            width=400 height=200
            background-image="$img(1 @1x, 2 @2x)"
            background-image-slice=%d
      }

      +view col4 class="flex flex-col gap-2" {
        +text l4 content="Tile" class="text-xs text-zinc-500"
        +view tile-2x class="rounded-xl overflow-clip"
            width=400 height=200
            background-image="$img(1 @1x, 2 @2x)"
            background-image-slice=%d
            background-image-repeat=tile
      }
    }
  }
\e\\' "$SLICE" "$SLICE" "$SLICE" "$SLICE"

# Keep alive
while true; do sleep 60; done
