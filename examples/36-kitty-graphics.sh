#!/bin/bash
# Bouncing BYO logo — Kitty Graphics Protocol + $img(N) backgrounds
#
# Demonstrates:
#   1. Uploading a PNG via the Kitty Terminal Graphics Protocol (chunked base64)
#   2. Displaying it on a view via background-image=$img(N)
#   3. Animating the view — bouncing DVD-screensaver style
#
# Run from the project root:  ./examples/36-kitty-graphics.sh

# ─── Kitty Graphics upload helpers ────────────────────────────────────────────
# Adapted from the Kitty terminal graphics protocol reference.
# Reads base64 chunks from stdin, sends them as kitty G frames.

send_chunked() {
    local first="y" id="$1"
    # "|| [ -n "$chunk" ]" handles last line without trailing newline
    while IFS= read -r chunk || [ -n "$chunk" ]; do
        if [ "$first" = "y" ]; then
            # First chunk: specify action, format, image id; q=1 suppresses OK
            printf '\e_Ga=t,f=100,q=1,i=%d,m=1;%s\e\\' "$id" "$chunk"
            first="n"
        else
            printf '\e_Gq=1,m=1;%s\e\\' "$chunk"
        fi
    done
    # Final empty chunk signals upload complete; q=2 suppresses all responses
    [ "$first" = "n" ] && printf '\e_Gq=2,m=0;\e\\'
}

transmit_png() {
    local id="$1" file="$2"
    # Try GNU base64 (-w), then BSD base64 (-b), then openssl fallback
    { base64 -w 4096 "$file" 2>/dev/null | send_chunked "$id"; } ||
    { base64 -b 4096 "$file" 2>/dev/null | send_chunked "$id"; } ||
    { openssl base64 -e -A -in "$file" | fold -b -w 4096 | send_chunked "$id"; }
}

# ─── Phase 1: Upload the BYO logo ────────────────────────────────────────────

transmit_png 1 assets/byo-logo.png

# Give the compositor a moment to decode and store the image
sleep 0.1

# ─── Phase 2: Create the scene ───────────────────────────────────────────────
# A dark background with the logo centered, plus a bounce counter.

LOGO_W=256
LOGO_H=128
START_X=100
START_Y=100

printf '\e_B
  +view root class="relative w-full h-full bg-zinc-950 overflow-clip" {
    +view logo class="absolute border-2 overflow-clip"
        width=%d height=%d left=%d top=%d
        background-image=$img(1)
        border-color="#34d399"
    +view hud class="absolute flex gap-4 items-center" right=16 bottom=16 {
      +text bounces content="Bounces: 0" class="text-sm text-zinc-500"
    }
  }
\e\\' "$LOGO_W" "$LOGO_H" "$START_X" "$START_Y"

sleep 2

# ─── Phase 3: Bouncing animation ─────────────────────────────────────────────
# Classic DVD screensaver: diagonal motion, bounce off walls.

# Assumes default 1280×720 window
SCREEN_W=1280
SCREEN_H=720

X=$START_X
Y=$START_Y
DX=4
DY=3
BOUNCES=0

MAX_X=$(( SCREEN_W - LOGO_W ))
MAX_Y=$(( SCREEN_H - LOGO_H ))

while true; do
    X=$(( X + DX ))
    Y=$(( Y + DY ))

    BOUNCE=0

    if [ $X -le 0 ] || [ $X -ge $MAX_X ]; then
        DX=$(( -DX ))
        X=$(( X + DX + DX ))
        BOUNCE=1
    fi

    if [ $Y -le 0 ] || [ $Y -ge $MAX_Y ]; then
        DY=$(( -DY ))
        Y=$(( Y + DY + DY ))
        BOUNCE=1
    fi

    if [ $BOUNCE -eq 1 ]; then
        BOUNCES=$(( BOUNCES + 1 ))
        printf '\e_B
          @view logo left=%d top=%d
          @text bounces content="Bounces: %d"
        \e\\' "$X" "$Y" "$BOUNCES"
    else
        printf '\e_B @view logo left=%d top=%d \e\\' "$X" "$Y"
    fi

    sleep 0.016
done
