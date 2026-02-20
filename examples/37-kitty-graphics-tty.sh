#!/bin/bash
# Bouncing image inside a TTY — Kitty Graphics cursor-position placements
#
# Demonstrates:
#   1. Uploading a PNG via the Kitty Terminal Graphics Protocol (chunked base64)
#   2. Using VT100 cursor positioning within the default root TTY
#   3. Placing the image at the cursor via `a=p` (put)
#   4. Animating by reusing the same placement ID (replaces in-place)
#
# Run from the project root:  ./examples/37-kitty-graphics-tty.sh

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

# ─── Phase 1: Upload the BYO logo ────────────────────────────────────────────

transmit_png 1 assets/byo-logo.png

sleep 0.1

# ─── Phase 2: Print some text via passthrough ────────────────────────────────
# The default root tty "/" receives passthrough, so plain echo output
# and VT100 escapes go directly to it — no views or redirects needed.

echo "BYO/OS — Kitty Graphics TTY Placement Demo"
echo ""
echo "Image bouncing via cursor-position placements (a=p)..."

sleep 0.5

# ─── Phase 3: Bouncing animation ─────────────────────────────────────────────
# VT100 cursor positioning + kitty `a=p` with the same placement ID (p=1)
# replaces the previous placement each frame, creating animation.
# No c=/r= specified — image displays at its native pixel size.

# Starting position (1-based VT100 coords)
COL=5
ROW=6
DCOL=1
DROW=1

MAX_COL=60
MAX_ROW=20

while true; do
    COL=$(( COL + DCOL ))
    ROW=$(( ROW + DROW ))

    if [ $COL -le 2 ] || [ $COL -ge $MAX_COL ]; then
        DCOL=$(( -DCOL ))
        COL=$(( COL + DCOL + DCOL ))
    fi

    if [ $ROW -le 5 ] || [ $ROW -ge $MAX_ROW ]; then
        DROW=$(( -DROW ))
        ROW=$(( ROW + DROW + DROW ))
    fi

    # Move cursor via VT100 CUP (passthrough → root tty)
    printf '\e[%d;%dH' "$ROW" "$COL"

    # Place image at cursor — p=1 replaces the existing placement
    printf '\e_Ga=p,i=1,p=1,q=2\e\\'

    sleep 0.033
done
