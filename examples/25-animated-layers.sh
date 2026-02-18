#!/bin/bash
# Animated 3D layers — rotating layers with PBR materials, spread apart
printf '\e_B
  +window scene {
    +layer card1 width=300 height=200 order=0
        unlit=false alpha-mode=blend
        class="roughness-50 metallic-50"
        translate-x="-350" rotate-y=0 {
      +view c1-root class="flex flex-col items-center justify-center w-full h-full bg-sky-600 rounded-xl" {
        +text c1-t1 content="Card A" class="text-3xl text-white"
        +text c1-t2 content="Rotating Y-axis" class="text-sky-200"
      }
    }

    +layer card2 width=300 height=200 order=1
        unlit=false alpha-mode=blend
        class="roughness-25 metallic-75"
        rotate-x=0 {
      +view c2-root class="flex flex-col items-center justify-center w-full h-full bg-emerald-600 rounded-xl" {
        +text c2-t1 content="Card B" class="text-3xl text-white"
        +text c2-t2 content="Rotating X-axis" class="text-emerald-200"
      }
    }

    +layer card3 width=300 height=200 order=2
        unlit=false alpha-mode=blend
        class="roughness-0 metallic-100"
        translate-x=350 rotate-z=0 {
      +view c3-root class="flex flex-col items-center justify-center w-full h-full bg-purple-600 rounded-xl" {
        +text c3-t1 content="Card C" class="text-3xl text-white"
        +text c3-t2 content="Rotating Z-axis" class="text-purple-200"
      }
    }
  }
\e\\'
sleep 0.5

T=0
while true; do
    # Card A: gentle Y-axis oscillation
    read -r RY <<< $(awk "BEGIN { printf \"%.1f\", 30 * sin($T * 3.14159 / 180) }")
    # Card B: gentle X-axis oscillation (offset phase)
    read -r RX <<< $(awk "BEGIN { printf \"%.1f\", 25 * sin(($T + 120) * 3.14159 / 180) }")
    # Card C: continuous Z rotation
    RZ=$(( T % 360 ))

    printf '\e_B
      @layer card1 rotate-y="%s"
      @layer card2 rotate-x="%s"
      @layer card3 rotate-z=%d
    \e\\' "$RY" "$RX" "$RZ"

    T=$(( T + 1 ))
    sleep 0.016
done
