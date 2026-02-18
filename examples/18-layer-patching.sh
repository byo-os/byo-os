#!/bin/bash
# Dynamic layer — counter + progress bar with live patching
printf '\e_B
  +window main {
    +layer hud width=1280 height=720 {
      +view root class="flex flex-col items-center justify-center w-full h-full bg-slate-900 gap-4" {
        +text counter content="Count: 0" class="text-3xl text-white"
        +view bar class="w-64 h-8 rounded-full bg-slate-700" {
          +view fill class="h-8 rounded-full bg-sky-500" width=0
        }
      }
    }
  }
\e\\'
sleep 1

N=0
while true; do
    N=$((N + 1))
    W=$((N % 101))
    printf '\e_B
      @text counter content="Count: %d"
      @view fill width="%d%%"
    \e\\' "$N" "$W"
    sleep 0.3
done
