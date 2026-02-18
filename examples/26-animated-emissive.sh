#!/bin/bash
# Animated emissive — pulsing glow effect on layers
printf '\e_B
  +window scene {
    +layer pulse1 width=250 height=250 order=0
        translate-x="-180"
        unlit=false alpha-mode=blend
        emissive-color="rgb(0, 0, 0)" {
      +view p1-root class="flex flex-col items-center justify-center w-full h-full bg-zinc-900 rounded-full" {
        +text p1-t1 content="PULSE" class="text-3xl text-red-400"
        +text p1-t2 content="Red Channel" class="text-zinc-500"
      }
    }

    +layer pulse2 width=250 height=250 order=1
        unlit=false alpha-mode=blend
        emissive-color="rgb(0, 0, 0)" {
      +view p2-root class="flex flex-col items-center justify-center w-full h-full bg-zinc-900 rounded-full" {
        +text p2-t1 content="PULSE" class="text-3xl text-emerald-400"
        +text p2-t2 content="Green Channel" class="text-zinc-500"
      }
    }

    +layer pulse3 width=250 height=250 order=2
        translate-x=180
        unlit=false alpha-mode=blend
        emissive-color="rgb(0, 0, 0)" {
      +view p3-root class="flex flex-col items-center justify-center w-full h-full bg-zinc-900 rounded-full" {
        +text p3-t1 content="PULSE" class="text-3xl text-sky-400"
        +text p3-t2 content="Blue Channel" class="text-zinc-500"
      }
    }
  }
\e\\'
sleep 0.5

T=0
while true; do
    # Sine-wave pulsing HDR glow — each channel offset by 120 degrees
    read -r R G B <<< $(awk "BEGIN {
        pi = 3.14159265;
        t = $T * pi / 90;
        r = int(255 + 255 * sin(t));
        g = int(255 + 255 * sin(t + 2*pi/3));
        b = int(255 + 255 * sin(t + 4*pi/3));
        if (r < 0) r = 0;
        if (g < 0) g = 0;
        if (b < 0) b = 0;
        printf \"%d %d %d\", r, g, b
    }")

    printf '\e_B
      @layer pulse1 emissive-color="rgb(%d, 0, 0)"
      @layer pulse2 emissive-color="rgb(0, %d, 0)"
      @layer pulse3 emissive-color="rgb(0, 0, %d)"
    \e\\' "$R" "$G" "$B"

    T=$(( T + 1 ))
    sleep 0.033
done
