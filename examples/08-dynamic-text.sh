#!/bin/bash
# Dynamic text updates — a clock-like display
printf '\e_B
  +view root class="flex flex-col items-center justify-center w-full h-full bg-zinc-900 gap-4" {
    +text clock content="00:00:00" class="text-2xl text-white"
    +text label content="Updating every second..." class="text-zinc-500"
  }
\e\\'

while true; do
    NOW=$(date +%H:%M:%S)
    printf '\e_B @text clock content="%s" \e\\' "$NOW"
    sleep 1
done
