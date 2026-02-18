#!/bin/bash
# Animated transforms — orbiting cards with sine/cosine motion
# Orbs are absolutely positioned within a flex-centered anchor, then translated.
printf '\e_B
  +view root class="flex items-center justify-center w-full h-full bg-zinc-950" {
    +view anchor class="relative" {
      +view center class="w-4 h-4 rounded-full bg-zinc-700"
      +view orb1 class="absolute w-20 h-20 rounded-xl bg-sky-500/80 flex items-center justify-center"
          left="-32" top="-32" translate-x=0 translate-y=0 {
        +text o1 content="1" class="text-white text-xl"
      }
      +view orb2 class="absolute w-20 h-20 rounded-xl bg-emerald-500/80 flex items-center justify-center"
          left="-32" top="-32" translate-x=0 translate-y=0 {
        +text o2 content="2" class="text-white text-xl"
      }
      +view orb3 class="absolute w-20 h-20 rounded-xl bg-purple-500/80 flex items-center justify-center"
          left="-32" top="-32" translate-x=0 translate-y=0 {
        +text o3 content="3" class="text-white text-xl"
      }
      +view orb4 class="absolute w-20 h-20 rounded-xl bg-amber-500/80 flex items-center justify-center"
          left="-32" top="-32" translate-x=0 translate-y=0 {
        +text o4 content="4" class="text-white text-xl"
      }
    }
    +text label content="Orbiting Views" class="absolute text-zinc-600 text-sm" bottom=32
  }
\e\\'
sleep 0.5

# Use awk for floating-point math
T=0
while true; do
    read -r X1 Y1 X2 Y2 X3 Y3 X4 Y4 <<< $(awk "BEGIN {
        r = 150;
        pi = 3.14159265;
        t = $T * pi / 180;
        printf \"%.1f %.1f %.1f %.1f %.1f %.1f %.1f %.1f\",
            r*cos(t),       r*sin(t),
            r*cos(t+pi/2),  r*sin(t+pi/2),
            r*cos(t+pi),    r*sin(t+pi),
            r*cos(t+3*pi/2), r*sin(t+3*pi/2)
    }")

    printf '\e_B
      @view orb1 translate-x="%s" translate-y="%s" rotate=%d
      @view orb2 translate-x="%s" translate-y="%s" rotate=%d
      @view orb3 translate-x="%s" translate-y="%s" rotate=%d
      @view orb4 translate-x="%s" translate-y="%s" rotate=%d
    \e\\' "$X1" "$Y1" "$T" "$X2" "$Y2" "$T" "$X3" "$Y3" "$T" "$X4" "$Y4" "$T"

    T=$(( (T + 1) % 360 ))
    sleep 0.016
done
