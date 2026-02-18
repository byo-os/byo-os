#!/bin/bash
# Animated progress bars
printf '\e_B
  +view root class="flex flex-col items-center justify-center w-full h-full bg-zinc-900 gap-6 p-16" {
    +text title content="Downloads" class="text-xl text-white"

    +view dl1 class="flex flex-col w-full gap-1" {
      +text dl1-name content="bevy-0.18.0.tar.gz" class="text-zinc-300"
      +view dl1-track class="w-full h-3 rounded-full bg-zinc-700" {
        +view dl1-fill class="h-3 rounded-full bg-sky-500" width=0
      }
      +text dl1-pct content="0%%" class="text-zinc-500"
    }

    +view dl2 class="flex flex-col w-full gap-1" {
      +text dl2-name content="rust-1.82.0-src.tar.xz" class="text-zinc-300"
      +view dl2-track class="w-full h-3 rounded-full bg-zinc-700" {
        +view dl2-fill class="h-3 rounded-full bg-emerald-500" width=0
      }
      +text dl2-pct content="0%%" class="text-zinc-500"
    }

    +view dl3 class="flex flex-col w-full gap-1" {
      +text dl3-name content="linux-6.8.tar.xz" class="text-zinc-300"
      +view dl3-track class="w-full h-3 rounded-full bg-zinc-700" {
        +view dl3-fill class="h-3 rounded-full bg-purple-500" width=0
      }
      +text dl3-pct content="0%%" class="text-zinc-500"
    }
  }
\e\\'
sleep 1

# Animate the three progress bars at different speeds
P1=0; P2=0; P3=0
SPEED1=7; SPEED2=4; SPEED3=2

while true; do
    P1=$((P1 + SPEED1)); [ $P1 -gt 100 ] && P1=100
    P2=$((P2 + SPEED2)); [ $P2 -gt 100 ] && P2=100
    P3=$((P3 + SPEED3)); [ $P3 -gt 100 ] && P3=100

    printf '\e_B
      @view dl1-fill width="%s%%"
      @text dl1-pct content="%d%%"
      @view dl2-fill width="%s%%"
      @text dl2-pct content="%d%%"
      @view dl3-fill width="%s%%"
      @text dl3-pct content="%d%%"
    \e\\' "$P1" "$P1" "$P2" "$P2" "$P3" "$P3"

    if [ $P1 -ge 100 ] && [ $P2 -ge 100 ] && [ $P3 -ge 100 ]; then
        printf '\e_B @text title content="All downloads complete!" \e\\'
        break
    fi

    sleep 0.2
done

sleep infinity
