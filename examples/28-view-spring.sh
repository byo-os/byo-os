#!/bin/bash
# Spring animation — bouncy views with scale and translate
printf '\e_B
  +view root class="flex flex-col items-center justify-center w-full h-full bg-zinc-950 gap-6" {
    +text title content="Spring Animation" class="text-2xl text-white"
    +view stage class="flex flex-row gap-4 items-center" {
      +view box1 class="w-16 h-16 rounded-lg bg-sky-500" scale=0 translate-y=0
      +view box2 class="w-16 h-16 rounded-lg bg-emerald-500" scale=0 translate-y=0
      +view box3 class="w-16 h-16 rounded-lg bg-purple-500" scale=0 translate-y=0
      +view box4 class="w-16 h-16 rounded-lg bg-amber-500" scale=0 translate-y=0
      +view box5 class="w-16 h-16 rounded-lg bg-rose-500" scale=0 translate-y=0
      +view box6 class="w-16 h-16 rounded-lg bg-teal-500" scale=0 translate-y=0
      +view box7 class="w-16 h-16 rounded-lg bg-indigo-500" scale=0 translate-y=0
      +view box8 class="w-16 h-16 rounded-lg bg-orange-500" scale=0 translate-y=0
    }
    +text hint content="Staggered spring entrance" class="text-zinc-600"
  }
\e\\'
sleep 0.8

# Staggered spring animation for each box
# Spring params: damping=0.6, frequency=3Hz, 8 boxes staggered by 80ms
FRAME=0
while true; do
    CMD='\e_B'
    ALL_DONE=true

    for i in $(seq 1 8); do
        # Each box starts 5 frames later than the previous
        LOCAL_FRAME=$(( FRAME - (i - 1) * 5 ))
        if [ $LOCAL_FRAME -lt 0 ]; then
            LOCAL_FRAME=0
            ALL_DONE=false
        fi

        read -r SCALE TY <<< $(awk "BEGIN {
            t = $LOCAL_FRAME / 60.0;
            if (t <= 0) { printf \"0.0 40.0\"; exit }
            # Damped spring
            d = 0.5; w = 18.0;
            env = exp(-d * w * t);
            s = 1.0 - env * cos(w * t * 0.8);
            y = -40.0 * env * cos(w * t * 0.6);
            if (s < 0) s = 0;
            printf \"%.3f %.1f\", s, y
        }")

        CMD="$CMD @view box$i scale=\"$SCALE\" translate-y=\"$TY\""

        # Check if settled
        if [ $LOCAL_FRAME -lt 120 ]; then
            ALL_DONE=false
        fi
    done

    printf '%s \e\\' "$CMD"

    FRAME=$(( FRAME + 1 ))
    if [ "$ALL_DONE" = true ] && [ $FRAME -gt 200 ]; then
        break
    fi
    sleep 0.016
done

sleep 99999
