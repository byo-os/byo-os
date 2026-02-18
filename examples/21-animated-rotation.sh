#!/bin/bash
# Animated rotation — spinning cards using patched rotate prop
printf '\e_B
  +view root class="flex flex-col items-center justify-center w-full h-full bg-zinc-950 gap-8" {
    +text title content="Animated Rotation" class="text-2xl text-white"
    +view stage class="flex flex-row gap-24 items-center" {
      +view spin1 class="flex items-center justify-center w-28 h-28 rounded-xl bg-sky-500" rotate=0 {
        +text s1 content="Slow" class="text-white"
      }
      +view spin2 class="flex items-center justify-center w-28 h-28 rounded-2xl bg-emerald-500" rotate=0 {
        +text s2 content="Medium" class="text-white"
      }
      +view spin3 class="flex items-center justify-center w-28 h-28 rounded-full bg-purple-500" rotate=0 {
        +text s3 content="Fast" class="text-white"
      }
    }
    +text hint content="Continuous rotation via @patch" class="text-zinc-600"
  }
\e\\'
sleep 0.5

ANGLE=0
while true; do
    ANGLE=$(( (ANGLE + 1) % 360 ))
    A2=$(( (ANGLE * 2) % 360 ))
    A3=$(( (ANGLE * 4) % 360 ))

    printf '\e_B
      @view spin1 rotate=%d
      @view spin2 rotate=%d
      @view spin3 rotate=%d
    \e\\' "$ANGLE" "$A2" "$A3"

    sleep 0.016
done
