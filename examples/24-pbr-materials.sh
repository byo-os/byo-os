#!/bin/bash
# PBR material showcase — roughness, metallic, emissive, transmission
# Background bars visible through glass; cards swing to show reflections
NAMES=("rough0" "rough25" "rough50" "rough100" "glow" "blue-glow" "gold" "clearcoat" "glass0" "glass25" "glass50" "glass75")

printf '\e_B
  +window gallery {

    +layer bar0 width=1100 height=60 translate-y=300 translate-z="-200"
        class="unlit alpha-opaque" {
      +view bar0-v class="w-full h-full bg-rose-500"
    }
    +layer bar1 width=1100 height=60 translate-y=100 translate-z="-200"
        class="unlit alpha-opaque" {
      +view bar1-v class="w-full h-full bg-emerald-500"
    }
    +layer bar2 width=1100 height=60 translate-y="-100" translate-z="-200"
        class="unlit alpha-opaque" {
      +view bar2-v class="w-full h-full bg-sky-500"
    }
    +layer bar3 width=1100 height=60 translate-y="-300" translate-z="-200"
        class="unlit alpha-opaque" {
      +view bar3-v class="w-full h-full bg-amber-500"
    }

    +layer rough0 width=200 height=200 order=0
        translate-x="-350" translate-y=240
        class="roughness-0 metallic-100 lit alpha-blend" {
      +view r0-root class="flex flex-col items-center justify-center w-full h-full bg-zinc-400" {
        +text r0-t1 content="Mirror" class="text-zinc-900 text-lg"
        +text r0-t2 content="roughness-0 metallic-100" class="text-zinc-700 text-xs"
      }
    }

    +layer rough25 width=200 height=200 order=1
        translate-x="-120" translate-y=240
        class="roughness-25 metallic-100 lit alpha-blend" {
      +view r25-root class="flex flex-col items-center justify-center w-full h-full bg-zinc-400" {
        +text r25-t1 content="Polished" class="text-zinc-900 text-lg"
        +text r25-t2 content="roughness-25 metallic-100" class="text-zinc-700 text-xs"
      }
    }

    +layer rough50 width=200 height=200 order=2
        translate-x=110 translate-y=240
        class="roughness-50 metallic-75 lit alpha-blend" {
      +view r50-root class="flex flex-col items-center justify-center w-full h-full bg-zinc-400" {
        +text r50-t1 content="Satin" class="text-zinc-900 text-lg"
        +text r50-t2 content="roughness-50 metallic-75" class="text-zinc-700 text-xs"
      }
    }

    +layer rough100 width=200 height=200 order=3
        translate-x=340 translate-y=240
        class="roughness-100 metallic-0 lit alpha-blend" {
      +view r100-root class="flex flex-col items-center justify-center w-full h-full bg-zinc-400" {
        +text r100-t1 content="Matte" class="text-zinc-900 text-lg"
        +text r100-t2 content="roughness-100 metallic-0" class="text-zinc-700 text-xs"
      }
    }

    +layer glow width=200 height=200 order=4 format=rgba16float
        translate-x="-350" translate-y=0
        class="unlit alpha-blend" {
      +view glow-root class="flex flex-col items-center justify-center w-full h-full bg-zinc-900" {
        +text glow-t1 content="HDR Text" class="text-lg" color="rgb(2000, 400, 100)"
        +text glow-t2 content="format=rgba16float" class="text-xs" color="rgb(600, 200, 50)"
      }
    }

    +layer blue-glow width=200 height=200 order=5
        translate-x="-120" translate-y=0
        class="lit alpha-blend emissive-[rgb(100,400,2000)]" {
      +view bg-root class="flex flex-col items-center justify-center w-full h-full bg-sky-500" {
        +text bg-t1 content="Blue Glow" class="text-white text-lg"
        +text bg-t2 content="emissive-[rgb(100,400,2000)]" class="text-sky-100 text-xs"
      }
    }

    +layer gold width=200 height=200 order=6
        translate-x=110 translate-y=0
        class="roughness-25 metallic-100 lit alpha-blend" {
      +view gold-root class="flex flex-col items-center justify-center w-full h-full bg-yellow-600" {
        +text gold-t1 content="Gold" class="text-yellow-100 text-lg"
        +text gold-t2 content="roughness-25 metallic-100" class="text-yellow-300 text-xs"
      }
    }

    +layer clearcoat width=200 height=200 order=7
        translate-x=340 translate-y=0
        class="roughness-25 metallic-50 clearcoat-100 lit alpha-blend" {
      +view cc-root class="flex flex-col items-center justify-center w-full h-full bg-indigo-700" {
        +text cc-t1 content="Clearcoat" class="text-indigo-100 text-lg"
        +text cc-t2 content="clearcoat-100" class="text-indigo-300 text-xs"
      }
    }

    +layer glass0 width=200 height=200 order=8
        translate-x="-350" translate-y="-240"
        class="roughness-0 metallic-0 lit alpha-opaque transmission-95 ior-[1.5] thickness-[20]" {
      +view g0-root class="flex flex-col items-center justify-center w-full h-full bg-zinc-50 border border-zinc-300" {
        +text g0-t1 content="Clear Glass" class="text-zinc-800 text-lg"
        +text g0-t2 content="transmission-95 roughness-0" class="text-zinc-600 text-xs"
      }
    }

    +layer glass25 width=200 height=200 order=9
        translate-x="-120" translate-y="-240"
        class="roughness-[0.06] metallic-0 lit alpha-opaque transmission-95 ior-[1.5] thickness-[20]" {
      +view g25-root class="flex flex-col items-center justify-center w-full h-full bg-zinc-50 border border-zinc-300" {
        +text g25-t1 content="Light Frost" class="text-zinc-800 text-lg"
        +text g25-t2 content="transmission-95 roughness-[0.06]" class="text-zinc-600 text-xs"
      }
    }

    +layer glass50 width=200 height=200 order=10
        translate-x=110 translate-y="-240"
        class="roughness-[0.12] metallic-0 lit alpha-opaque transmission-95 ior-[1.5] thickness-[20]" {
      +view g50-root class="flex flex-col items-center justify-center w-full h-full bg-zinc-50 border border-zinc-300" {
        +text g50-t1 content="Frosted" class="text-zinc-800 text-lg"
        +text g50-t2 content="transmission-95 roughness-[0.12]" class="text-zinc-600 text-xs"
      }
    }

    +layer glass75 width=200 height=200 order=11
        translate-x=340 translate-y="-240"
        class="roughness-[0.2] metallic-0 lit alpha-opaque transmission-95 ior-[1.5] thickness-[20]" {
      +view g75-root class="flex flex-col items-center justify-center w-full h-full bg-zinc-50 border border-zinc-300" {
        +text g75-t1 content="Dense Frost" class="text-zinc-800 text-lg"
        +text g75-t2 content="transmission-95 roughness-[0.2]" class="text-zinc-600 text-xs"
      }
    }
  }
\e\\'
sleep 0.5

# Diagonal swing — rotate around top-left→bottom-right axis
# Each card is phase-delayed so they ripple across the grid
T=0
while true; do
    for i in $(seq 0 11); do
        PHASE=$((i * 8))
        read -r RX RY <<< $(awk "BEGIN {
            pi = 3.14159265;
            angle = 30 * sin(($T + $PHASE) * pi / 45);
            printf \"%.1f %.1f\", angle, -angle
        }")
        printf '\e_B @layer %s rotate-x="%s" rotate-y="%s" \e\\' \
            "${NAMES[$i]}" "$RX" "$RY"
    done

    T=$((T + 1))
done
